// NOT using linfa arima library
// Lightweight ARIMA fitting & forecasting module for constrained devices (Kindle Paperwhite 3).
// Accepts Vec<ChapterInformation> where each item has:
//   - chapter_number: Option<Decimal>
//   - last_updated: Option<i64> (unix seconds)
// The module:
//   - Preprocesses the chapter vector (filter/normalize/sort by chapter_number)
//   - Builds a timestamp series (ascending) and optionally uses a rolling window
//   - Fits a light conditional least squares ARIMA(p,d,q) with coordinate descent
//   - Forecasts 1-step ahead (timestamp in seconds)
// Comments are in English; code avoids heavy dependencies for suitability on low-power devices.
//
// NOTE: If you don't want Decimal dependency, change ChapterInformation.chapter_number to Option<f64>.
// To enable Decimal, add to Cargo.toml:
//   rust_decimal = "1.30"
//
// Typical usage:
//   let model = fit_arima_from_chapters(&chapters, ArimaSpec::default());
//   let next_ts = model.and_then(|m| m.forecast_1_from_chapters(&chapters));
//

use anyhow::bail;
use rust_decimal::prelude::ToPrimitive;
use std::cmp::Ordering;

use crate::model::ChapterInformation;

/// ARIMA specification
#[derive(Debug, Clone, Copy)]
pub struct ArimaSpec {
    pub p: usize,
    pub d: usize,
    pub q: usize,
    /// Rolling window length: if Some(n), fit only last n timestamps (after filtering/sorting).
    /// Useful to ignore very-old history which may be irrelevant or contain outliers.
    pub rolling_window: Option<usize>,
    /// Minimum points required to attempt fit (after differencing).
    pub min_points: usize,
}

impl Default for ArimaSpec {
    fn default() -> Self {
        Self {
            p: 1,
            d: 1,
            q: 1,
            rolling_window: Some(30), // <-- default changed to 30 as requested
            min_points: 6,
        }
    }
}

/// Fitted model returned by fit function
#[derive(Debug, Clone)]
pub struct ArimaModel {
    pub spec: ArimaSpec,
    pub ar: Vec<f64>, // length p
    pub ma: Vec<f64>, // length q
    pub mu: f64,      // drift in differenced series
    pub sigma2: f64,  // residual variance
    pub aic: f64,
    pub bic: f64,
}

impl ArimaModel {
    /// Forecast next timestamp directly from original ChapterInformation slice (works with Option fields)
    /// Returns unix timestamp seconds rounded to i64.
    pub fn forecast_1_from_chapters(
        &self,
        chapters: &[ChapterInformation],
        last_check_no_update: Option<i64>,
    ) -> Option<i64> {
        let ts = timestamps_from_chapters(chapters, self.spec.rolling_window)?;
        if ts.is_empty() {
            return None;
        }

        let ts_f64: Vec<f64> = ts.iter().map(|x| *x as f64).collect();
        let raw_forecast = self.forecast_1(&ts_f64)?;

        let mut predicted_ts = raw_forecast.round() as i64;

        if let Some(last_check) = last_check_no_update {
            if predicted_ts <= last_check {
                let interval = if self.mu > 3600.0 {
                    self.mu
                } else {
                    calculate_recent_interval(&ts_f64, 5).unwrap_or(86400.0)
                };

                predicted_ts = last_check + interval.round() as i64;
            }
        }

        Some(predicted_ts)
    }

    /// Forecast next timestamp from a numeric timestamp slice (ascending)
    pub fn forecast_1(&self, original_ts: &[f64]) -> Option<f64> {
        if original_ts.len() < 2 {
            return None;
        }

        // @1 difference series d times
        let diff = difference_series(original_ts, self.spec.d);
        if diff.is_empty() {
            return None;
        }

        // @2 compute residuals to retrieve last errors
        let (resids, _fitted) = compute_residuals_and_fitted(&diff, &self.ar, &self.ma, self.mu);

        // prepare last p differenced values (if not enough, fill with zeros)
        let n = diff.len();
        let mut d_vals = vec![0.0; self.ar.len()];
        for i in 0..self.ar.len() {
            if i < n {
                d_vals[i] = diff[n - 1 - i];
            }
        }

        // prepare last q residuals (if not enough, fill with zeros)
        let mut e_vals = vec![0.0; self.ma.len()];
        for i in 0..self.ma.len() {
            if i < resids.len() {
                e_vals[i] = resids[resids.len() - 1 - i];
            }
        }

        // compute next differenced value
        let mut next_diff = self.mu;
        for (i, &phi) in self.ar.iter().enumerate() {
            next_diff += phi * d_vals[i];
        }
        for (i, &theta) in self.ma.iter().enumerate() {
            next_diff += theta * e_vals[i];
        }

        // integrate back to timestamp (single integration per d)
        let mut next_ts = original_ts[original_ts.len() - 1] + next_diff;

        // If d >= 2, we do a simple cumulative integration using available last differences.
        // This is a pragmatic approach for small d (<=2) and short series.
        if self.spec.d >= 2 {
            // reconstruct second integration by adding last previous difference if available
            // For more precise behavior you'd keep full integrated states; this keeps code simple.
            if original_ts.len() >= 2 {
                // last first-difference:
                let last_diff =
                    original_ts[original_ts.len() - 1] - original_ts[original_ts.len() - 2];
                next_ts = original_ts[original_ts.len() - 1] + last_diff + next_diff;
            }
        }

        Some(next_ts)
    }
}
fn calculate_recent_interval(ts: &[f64], n_last: usize) -> Option<f64> {
    if ts.len() < 2 {
        return None;
    }
    let start = if ts.len() > n_last {
        ts.len() - n_last
    } else {
        0
    };
    let slice = &ts[start..];

    let mut sum_diff = 0.0;
    let mut count = 0;
    for i in 1..slice.len() {
        sum_diff += slice[i] - slice[i - 1];
        count += 1;
    }

    if count == 0 {
        None
    } else {
        Some(sum_diff / count as f64)
    }
}
// /// Estimate initial MA(1) using lag-1 autocovariance (simple)
// fn simple_ma1_initial(diff: &[f64]) -> f64 {
//     if diff.len() < 3 {
//         return 0.0;
//     }
//     let n = diff.len();
//     let mu = mean(diff);
//     let mut num = 0.0;
//     let mut den = 0.0;
//     for t in 1..n {
//         num += (diff[t] - mu) * (diff[t - 1] - mu);
//         den += (diff[t - 1] - mu).powi(2);
//     }
//     let rho = if den.abs() < 1e-12 { 0.0 } else { num / den };
//     // Rough relation AR(1) autocorrelation -> MA(1)
//     let theta = 0.5 * rho; // conservative
//     theta.clamp(-0.98, 0.98)
// }

/// PUBLIC API
/// Fit ARIMA model from ChapterInformation Vec.
/// Returns Option<ArimaModel> if fit successful.
pub fn fit_arima_from_chapters(
    chapters: &[ChapterInformation],
    spec: ArimaSpec,
) -> Result<ArimaModel, anyhow::Error> {
    // @1 extract timestamps sorted ascending
    let ts = timestamps_from_chapters(chapters, spec.rolling_window).unwrap_or([].into());

    // Ensure enough points
    if ts.len() < spec.min_points {
        bail!(
            "Warning: Not enough points ({} < {}) to fit ARIMA",
            ts.len(),
            spec.min_points
        )
    }

    // convert to f64
    let ts_f64: Vec<f64> = ts.iter().map(|x| *x as f64).collect();

    // @2 differenced series
    let diff = difference_series(&ts_f64, spec.d);
    if diff.len() < 3 {
        bail!(
            "Differenced series too short ({} points) to fit ARIMA",
            diff.len()
        )
    }

    // @3 initial parameters
    let mut ar_init = vec![0.0; spec.p];
    let mut ma_init = vec![0.0; spec.q];

    // AR init using lag-1 autocorrelation
    if spec.p >= 1 {
        ar_init[0] = simple_autoregressive_initial(&diff);
        // ma_init[0] = simple_ma1_initial(&diff);
        if spec.p == 2 {
            ar_init[1] = 0.0;
        }
    }

    // MA init using last residual autocorrelation (simple approx)
    if spec.q >= 1 {
        let resids = compute_residuals_and_fitted(&diff, &ar_init, &ma_init, 0.0).0;
        if !resids.is_empty() {
            let n = resids.len();
            let mut num = 0.0;
            let mut den = 0.0;
            for i in 1..n {
                num += resids[i] * resids[i - 1];
                den += resids[i - 1] * resids[i - 1];
            }
            ma_init[0] = if den.abs() > 1e-9 {
                (num / den).clamp(-0.98, 0.98)
            } else {
                0.0
            };
        }
    }

    // mu init small drift
    let mu_init = mean(&diff) * 0.01;

    // param vector layout: [ar..., ma..., mu]
    let mut params: Vec<f64> = Vec::with_capacity(spec.p + spec.q + 1);
    params.extend_from_slice(&ar_init);
    params.extend_from_slice(&ma_init);
    params.push(mu_init);

    // bounds
    let lower = vec![-0.99_f64; params.len()];
    let upper = vec![0.99_f64; params.len()];

    // Optimize: AR step default, MA step slightly larger
    let optimized = coordinate_descent_optimize_weighted(
        &diff, spec.p, spec.q, params, &lower, &upper, 350,  // max_iter
        1e-7, // tol
        1.5,  // MA step multiplier (increase movement for MA)
    );

    // Parse optimized back
    let mut idx = 0usize;
    let ar_opt = optimized[idx..idx + spec.p].to_vec();
    idx += spec.p;
    let ma_opt = optimized[idx..idx + spec.q].to_vec();
    idx += spec.q;
    let mu_opt = optimized[idx];

    // residuals
    let (resids, _fitted_vals) = compute_residuals_and_fitted(&diff, &ar_opt, &ma_opt, mu_opt);
    let n = resids.len() as f64;
    let sigma2 = if n > 0.0 {
        resids.iter().map(|r| r * r).sum::<f64>() / n
    } else {
        0.0
    };

    let sse = resids.iter().map(|r| r * r).sum::<f64>();
    let k = optimized.len() as f64;
    let aic = if n > 0.0 {
        n * (sse / n).ln() + 2.0 * k
    } else {
        f64::INFINITY
    };
    let bic = if n > 0.0 {
        n * (sse / n).ln() + k * n.ln()
    } else {
        f64::INFINITY
    };

    Ok(ArimaModel {
        spec,
        ar: ar_opt,
        ma: ma_opt,
        mu: mu_opt,
        sigma2,
        aic,
        bic,
    })
}

/// Weighted coordinate descent for ARIMA (recent residuals have more weight)
fn coordinate_descent_optimize_weighted(
    diff: &[f64],
    p: usize,
    q: usize,
    init_params: Vec<f64>,
    lower: &[f64],
    upper: &[f64],
    max_iter: usize,
    tol: f64,
    ma_step_multiplier: f64, // <--- new
) -> Vec<f64> {
    let mut params = init_params;
    let nparam = params.len();
    let mut last_loss = sse_loss_weighted(diff, p, q, &params);

    for _it in 0..max_iter {
        let mut improved = false;

        for j in 0..nparam {
            let cur = params[j];
            let step = if j >= p {
                0.05_f64 * ma_step_multiplier
            } else {
                0.05_f64
            };
            let step = step.max(cur.abs() * 0.2).min(0.5);

            let left = (cur - step).max(lower[j]);
            let right = (cur + step).min(upper[j]);

            let mut p_left = params.clone();
            let mut p_right = params.clone();
            p_left[j] = left;
            p_right[j] = right;

            let f_left = sse_loss_weighted(diff, p, q, &p_left);
            let f_cur = last_loss;
            let f_right = sse_loss_weighted(diff, p, q, &p_right);

            // parabolic interpolation
            let x1 = left;
            let f1 = f_left;
            let x2 = cur;
            let f2 = f_cur;
            let x3 = right;
            let f3 = f_right;

            let denom = (x1 - x2) * (x1 - x3) * (x2 - x3);
            let mut x_vertex = cur;
            if denom.abs() > 1e-15 {
                let a = ((f1 * (x2 - x3)) + (f2 * (x3 - x1)) + (f3 * (x1 - x2))) / denom;
                let b = ((f1 * (x3 * x3 - x2 * x2))
                    + (f2 * (x1 * x1 - x3 * x3))
                    + (f3 * (x2 * x2 - x1 * x1)))
                    / denom;
                if a.abs() > 1e-20 {
                    x_vertex = -b / (2.0 * a);
                    x_vertex = x_vertex.clamp(lower[j], upper[j]);
                }
            }

            let mut p_cand = params.clone();
            p_cand[j] = x_vertex;
            let f_cand = sse_loss_weighted(diff, p, q, &p_cand);

            // choose best
            let mut best_x = cur;
            let mut best_f = f_cur;
            if f_left < best_f {
                best_f = f_left;
                best_x = left;
            }
            if f_right < best_f {
                best_f = f_right;
                best_x = right;
            }
            if f_cand < best_f {
                best_f = f_cand;
                best_x = x_vertex;
            }

            if best_f + 1e-12 < last_loss {
                params[j] = best_x;
                last_loss = best_f;
                improved = true;
            }
        }

        if !improved || last_loss < tol {
            break;
        }
    }

    params
}

/// SSE loss weighted for recent residuals (improves MA learning)
fn sse_loss_weighted(diff: &[f64], p: usize, q: usize, params: &[f64]) -> f64 {
    let mut idx = 0usize;
    let ar = &params[idx..idx + p];
    idx += p;
    let ma = &params[idx..idx + q];
    idx += q;
    let mu = params[idx];

    let (resids, _fitted) = compute_residuals_and_fitted(diff, ar, ma, mu);
    let n = resids.len() as f64;

    resids
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let weight = 1.0 + (i as f64 / n); // recent residuals weighted more
            r * r * weight
        })
        .sum()
}

/// ---------- Helper utilities ----------

/// Extracts timestamps from chapters:
/// - filters out items with missing last_updated
/// - sorts by chapter_number (if available) ascending; if chapter_number missing uses last_updated ascending
/// - deduplicates identical timestamps (keeps last occurrence)
/// - optionally applies rolling window (keeps last N)
fn timestamps_from_chapters(
    chapters: &[ChapterInformation],
    rolling_window: Option<usize>,
) -> Option<Vec<i64>> {
    // Collect tuples: (chapter_number_opt, last_updated)
    let mut items: Vec<(Option<f64>, i64)> = Vec::with_capacity(chapters.len());
    for ch in chapters.iter() {
        if let Some(ts) = ch.last_updated {
            // use chapter_number_as_f64 for ordering if available
            let cn = ch.chapter_number.map(|v| v.to_f64()).unwrap_or(Some(0.0));
            items.push((cn, ts));
        }
    }

    if items.is_empty() {
        return None;
    }

    // sort by chapter_number where available, otherwise by timestamp
    items.sort_by(|a, b| match (a.0, b.0) {
        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => a.1.cmp(&b.1),
    });

    // We want timestamps ascending. Many sources may present descending input.
    // After sorting by chapter number ascending, ensure timestamps ascending by stable sort.
    // If there are equal chapter numbers, order by timestamp ascending.
    items.sort_by(|a, b| {
        if let (Some(x), Some(y)) = (a.0, b.0) {
            let ord = x.partial_cmp(&y).unwrap_or(Ordering::Equal);
            if ord == Ordering::Equal {
                a.1.cmp(&b.1)
            } else {
                ord
            }
        } else {
            a.1.cmp(&b.1)
        }
    });

    // deduplicate identical timestamps: keep last occurrence
    let mut ts_vec: Vec<i64> = Vec::with_capacity(items.len());
    for (_, ts) in items.into_iter() {
        if ts_vec.last().copied() != Some(ts) {
            ts_vec.push(ts);
        }
    }

    // ensure ascending order by timestamp (safety)
    ts_vec.sort_unstable();

    // rolling window: keep last N timestamps (most recent)
    if let Some(n) = rolling_window {
        if ts_vec.len() > n {
            let start = ts_vec.len() - n;
            ts_vec = ts_vec[start..].to_vec();
        }
    }

    Some(ts_vec)
}

/// compute simple mean
fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        0.0
    } else {
        xs.iter().sum::<f64>() / (xs.len() as f64)
    }
}
/// Apply differencing d times.
/// Returns an empty Vec if differencing fully collapses the data.
/// This function is intentionally defensive because ARIMA(d) on short
/// or highly-collapsed timestamps may quickly reduce to length < 1.
fn difference_series(series: &[f64], d: usize) -> Vec<f64> {
    // If no differencing, simply clone
    if d == 0 {
        return series.to_vec();
    }

    // Start with original
    let mut out = series.to_vec();

    for _ in 0..d {
        if out.len() < 2 {
            // Not enough points to difference further
            return Vec::new();
        }

        // Produce first-difference series
        let mut diff = Vec::with_capacity(out.len() - 1);

        // Compute Δx_t = x_t - x_(t-1)
        for i in 1..out.len() {
            diff.push(out[i] - out[i - 1]);
        }

        out = diff;
    }

    out
}

/// Estimate initial AR(1) phi by lag-1 autocorrelation
fn simple_autoregressive_initial(diff: &[f64]) -> f64 {
    if diff.len() < 3 {
        return 0.0;
    }
    let n = diff.len();
    let mu = mean(diff);
    let mut num = 0.0;
    let mut den = 0.0;
    for i in 1..n {
        num += (diff[i] - mu) * (diff[i - 1] - mu);
        den += (diff[i - 1] - mu) * (diff[i - 1] - mu);
    }
    if den.abs() < 1e-12 {
        0.0
    } else {
        let phi = num / den;
        phi.clamp(-0.98, 0.98)
    }
}

/// Compute residuals and fitted values for differenced series given params.
/// Conditional sum of squares: pre-sample residuals assumed 0.
/// Returns (residuals, fitted_values)
fn compute_residuals_and_fitted(
    diff: &[f64],
    ar: &[f64],
    ma: &[f64],
    mu: f64,
) -> (Vec<f64>, Vec<f64>) {
    let n = diff.len();
    let p = ar.len();
    let q = ma.len();

    let mut resids = vec![0.0_f64; n];
    let mut fitted = vec![0.0_f64; n];

    for t in 0..n {
        let mut ar_part = 0.0;
        for i in 0..p {
            if t >= i + 1 {
                ar_part += ar[i] * diff[t - 1 - i];
            }
        }
        let mut ma_part = 0.0;
        for i in 0..q {
            if t >= i + 1 {
                ma_part += ma[i] * resids[t - 1 - i];
            }
        }
        let pred = mu + ar_part + ma_part;
        fitted[t] = pred;
        resids[t] = diff[t] - pred;
    }

    (resids, fitted)
}

#[cfg(test)]
mod tests {
    use crate::model::ChapterId;

    use super::*;

    use rust_decimal::prelude::FromPrimitive;
    use rust_decimal::Decimal;

    fn build_chapters(nums: Vec<Option<f64>>, ts: Vec<Option<i64>>) -> Vec<ChapterInformation> {
        nums.into_iter()
            .zip(ts.into_iter())
            .map(|(cn, ts)| ChapterInformation {
                chapter_number: cn.map(|v| Decimal::from_f64(v).unwrap()),
                last_updated: ts,
                id: ChapterId::from_strings("".to_owned(), "".to_owned(), "".to_owned()),
                title: Some("".to_owned()),
                scanlator: Some("".to_owned()),
                volume_number: Some(1.into()),
            })
            .collect()
    }

    #[test]
    fn test_difference_and_simple_fit() {
        // synthetic daily timestamps with small noise
        let base_start = 1_600_000_000i64;
        let mut timestamps: Vec<i64> = (0..40).map(|i| base_start + i * 86400).collect();
        timestamps[5] += 3600; // +1 hour jitter
        timestamps[20] -= 7200; // -2 hours jitter

        let nums = (1..=40).map(|i| Some(i as f64)).collect();
        let ts_opts = timestamps.iter().map(|&t| Some(t)).collect();

        let chapters = build_chapters(nums, ts_opts);

        let spec = ArimaSpec {
            p: 1,
            d: 1,
            q: 1,
            rolling_window: Some(30),
            min_points: 6,
        };
        let maybe_model = fit_arima_from_chapters(&chapters, spec);
        assert!(maybe_model.is_ok());
        let model = maybe_model.unwrap();
        assert_eq!(model.ar.len(), 1);
        // forecast should be near next day
        let pred = model.forecast_1_from_chapters(&chapters, None).unwrap();
        let expected_next = timestamps.last().unwrap() + 86400;
        let diff_abs = (pred - expected_next).abs();
        assert!(diff_abs < 3 * 86400);
    }

    #[test]
    fn test_preprocess_sorting() {
        let chapters = vec![
            ChapterInformation {
                chapter_number: Some(Decimal::from_f64(2.0).unwrap()),
                last_updated: Some(200),

                id: ChapterId::from_strings("".to_owned(), "".to_owned(), "".to_owned()),
                title: Some("".to_owned()),
                scanlator: Some("".to_owned()),
                volume_number: Some(1.into()),
            },
            ChapterInformation {
                chapter_number: None,
                last_updated: Some(100),

                id: ChapterId::from_strings("".to_owned(), "".to_owned(), "".to_owned()),
                title: Some("".to_owned()),
                scanlator: Some("".to_owned()),
                volume_number: Some(1.into()),
            },
            ChapterInformation {
                chapter_number: Some(Decimal::from_f64(1.0).unwrap()),
                last_updated: Some(150),
                id: ChapterId::from_strings("".to_owned(), "".to_owned(), "".to_owned()),
                title: Some("".to_owned()),
                scanlator: Some("".to_owned()),
                volume_number: Some(1.into()),
            },
            ChapterInformation {
                chapter_number: Some(Decimal::from_f64(2.0).unwrap()),
                last_updated: Some(250),
                id: ChapterId::from_strings("".to_owned(), "".to_owned(), "".to_owned()),
                title: Some("".to_owned()),
                scanlator: Some("".to_owned()),
                volume_number: Some(1.into()),
            },
            ChapterInformation {
                chapter_number: None,
                last_updated: None,
                id: ChapterId::from_strings("".to_owned(), "".to_owned(), "".to_owned()),
                title: Some("".to_owned()),
                scanlator: Some("".to_owned()),
                volume_number: Some(1.into()),
            },
        ];
        let ts = timestamps_from_chapters(&chapters, None).unwrap();
        assert!(ts.windows(2).all(|w| w[0] <= w[1]));
        assert!(ts.len() >= 3);
    }
}
