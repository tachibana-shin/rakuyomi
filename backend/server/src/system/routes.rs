use std::fs;
use std::path::Path;

use axum::extract::State as StateExtractor;
use axum::routing::get;
use axum::{Json, Router};
use log::warn;
use serde::Serialize;
use tokio::time::{sleep, Duration};

use crate::state::State;

pub fn routes() -> Router<State> {
    Router::<State>::new()
        .route("/system/stats", get(system_stats))
        .route("/system/startup-log", get(startup_log))
}

#[derive(Serialize)]
struct StartupLogResponse {
    messages: Vec<String>,
}

async fn startup_log(
    StateExtractor(State { startup_log, .. }): StateExtractor<State>,
) -> Json<StartupLogResponse> {
    let messages = startup_log.drain().await;
    Json(StartupLogResponse { messages })
}

#[derive(Serialize)]
struct CpuInfo {
    model: String,
    cores: usize,
    usage_percent: f64,
}

#[derive(Serialize)]
struct MemoryInfo {
    total_bytes: u64,
    available_bytes: u64,
    used_bytes: u64,
}

#[derive(Serialize)]
struct FilesystemInfo {
    path: String,
    total_bytes: u64,
    used_bytes: u64,
    free_bytes: u64,
}

#[derive(Serialize)]
struct ProcessInfo {
    memory_rss_bytes: u64,
    memory_virtual_bytes: u64,
}

#[derive(Serialize)]
struct SystemStats {
    cpu: CpuInfo,
    memory: MemoryInfo,
    tmpfs: Option<FilesystemInfo>,
    tmpfs_mount_error: Option<String>,
    storage: FilesystemInfo,
    process: ProcessInfo,
}

fn read_cpuinfo() -> Result<(String, usize), String> {
    let data = fs::read_to_string("/proc/cpuinfo").map_err(|e| e.to_string())?;
    let mut model = String::from("unknown");
    let mut cores = 0usize;

    for line in data.lines() {
        if let Some(val) = line.strip_prefix("model name\t: ") {
            if model == "unknown" {
                model = val.trim().to_string();
            }
        }
        if line.starts_with("processor") {
            cores += 1;
        }
    }

    Ok((model, cores))
}

fn cpu_usage_snapshot() -> Result<(u64, u64), String> {
    let data = fs::read_to_string("/proc/stat").map_err(|e| e.to_string())?;
    let first = data.lines().next().ok_or("empty /proc/stat")?;

    let parts: Vec<u64> = first
        .split_whitespace()
        .skip(1)
        .filter_map(|s| s.parse().ok())
        .collect();

    if parts.len() < 4 {
        return Err("not enough fields in /proc/stat cpu line".into());
    }

    // user + nice + system + idle + iowait + irq + softirq + steal
    let total: u64 = parts.iter().sum();
    let idle = parts[3];

    Ok((total, idle))
}

async fn cpu_usage_percent() -> f64 {
    let snap1 = match cpu_usage_snapshot() {
        Ok(v) => v,
        Err(e) => {
            warn!("failed to read CPU stats: {e}");
            return 0.0;
        }
    };

    sleep(Duration::from_millis(200)).await;

    let snap2 = match cpu_usage_snapshot() {
        Ok(v) => v,
        Err(e) => {
            warn!("failed to read CPU stats: {e}");
            return 0.0;
        }
    };

    let total_delta = snap2.0.saturating_sub(snap1.0);
    let idle_delta = snap2.1.saturating_sub(snap1.1);

    if total_delta == 0 {
        return 0.0;
    }

    (total_delta.saturating_sub(idle_delta) as f64 / total_delta as f64) * 100.0
}

fn read_meminfo() -> Result<(u64, u64), String> {
    let data = fs::read_to_string("/proc/meminfo").map_err(|e| e.to_string())?;
    let mut total = 0u64;
    let mut available = 0u64;

    for line in data.lines() {
        if let Some(val) = line.strip_prefix("MemTotal:") {
            total = parse_kb_value(val);
        }
        if let Some(val) = line.strip_prefix("MemAvailable:") {
            available = parse_kb_value(val);
        }
    }

    Ok((total, available))
}

fn parse_kb_value(s: &str) -> u64 {
    let val: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    val.parse::<u64>().unwrap_or(0) * 1024
}

fn read_process_status() -> Result<(u64, u64), String> {
    let data = fs::read_to_string("/proc/self/status").map_err(|e| e.to_string())?;
    let mut rss = 0u64;
    let mut vmsize = 0u64;

    for line in data.lines() {
        if let Some(val) = line.strip_prefix("VmRSS:") {
            rss = parse_kb_value(val);
        }
        if let Some(val) = line.strip_prefix("VmSize:") {
            vmsize = parse_kb_value(val);
        }
    }

    Ok((rss, vmsize))
}

fn read_filesystem_info(path: &Path) -> Result<FilesystemInfo, String> {
    #[cfg(feature = "api_18")]
    let (total, free, used) = unsafe {
        use std::ffi::CString;
        use std::mem::MaybeUninit;

        let c_path = CString::new(path.display().to_string()).map_err(|e| e.to_string())?;

        let mut stat = MaybeUninit::<libc::statfs>::uninit();

        if libc::statfs(c_path.as_ptr(), stat.as_mut_ptr()) == 0 {
            let stat = stat.assume_init();

            let bsize = stat.f_bsize as u64;
            let total = (stat.f_blocks as u64) * bsize;
            let free = (stat.f_bfree as u64) * bsize;
            let used = total.saturating_sub(free);

            (total, free, used)
        } else {
            return Err("Android statfs failed".to_string());
        }
    };

    #[cfg(not(feature = "api_18"))]
    let (total, free, used) = {
        let stat = nix::sys::statvfs::statvfs(path).map_err(|e| e.to_string())?;
        let frsize = stat.fragment_size() as u64;
        let total = (stat.blocks() as u64) * frsize;
        let free = (stat.blocks_free() as u64) * frsize;
        let used = total.saturating_sub(free);
        (total, free, used)
    };

    Ok(FilesystemInfo {
        path: path.display().to_string(),
        total_bytes: total,
        used_bytes: used,
        free_bytes: free,
    })
}

async fn system_stats(
    StateExtractor(State {
        chapter_storage, ..
    }): StateExtractor<State>,
) -> Result<Json<SystemStats>, crate::AppError> {
    let (downloads_path, is_ram_enabled, tmpfs_path, tmpfs_mount_error) = {
        let cs = chapter_storage.lock().await;
        (
            cs.downloads_path().clone(),
            cs.is_ram_enabled(),
            cs.tmpfs_path(),
            cs.tmpfs_mount_error().map(|s| s.to_string()),
        )
    };

    let (cpu_model, cpu_cores) = read_cpuinfo().unwrap_or_else(|e| {
        warn!("failed to read cpuinfo: {e}");
        (String::from("unknown"), 0)
    });

    let cpu_usage = cpu_usage_percent().await;

    let (mem_total, mem_avail) = read_meminfo().unwrap_or_else(|e| {
        warn!("failed to read meminfo: {e}");
        (0, 0)
    });

    let (proc_rss, proc_vm) = read_process_status().unwrap_or_else(|e| {
        warn!("failed to read process status: {e}");
        (0, 0)
    });

    // Storage path: settings.storage_path or default
    let storage = read_filesystem_info(&downloads_path)
        .map_err(|e| crate::AppError::Other(anyhow::anyhow!("failed to stat storage: {e}")))?;

    let tmpfs = if is_ram_enabled {
        if tmpfs_path.exists() {
            match read_filesystem_info(&tmpfs_path) {
                Ok(info) => Some(info),
                Err(e) => {
                    warn!("failed to stat tmpfs: {e}");
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    Ok(Json(SystemStats {
        cpu: CpuInfo {
            model: cpu_model,
            cores: cpu_cores,
            usage_percent: cpu_usage,
        },
        memory: MemoryInfo {
            total_bytes: mem_total,
            available_bytes: mem_avail,
            used_bytes: mem_total.saturating_sub(mem_avail),
        },
        tmpfs_mount_error,
        tmpfs,
        storage,
        process: ProcessInfo {
            memory_rss_bytes: proc_rss,
            memory_virtual_bytes: proc_vm,
        },
    }))
}
