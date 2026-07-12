local ffi = require("ffi")

ffi.cdef [[
  int pipe(int pipefd[2]);
  int close(int fd);
  ptrdiff_t read(int fd, void *buf, size_t count);
  int waitpid(int pid, int *wstatus, int options);
  char *getcwd(char *buf, size_t size);
  int chdir(const char *path);

  typedef struct { char __pad[128]; } posix_spawn_file_actions_t;
  int posix_spawnp(int *pid, const char *file, const posix_spawn_file_actions_t *file_actions, const void *attrp, const char *const argv[], char *const envp[]);
  int posix_spawn_file_actions_init(posix_spawn_file_actions_t *actions);
  int posix_spawn_file_actions_adddup2(posix_spawn_file_actions_t *actions, int fd, int newfd);
  int posix_spawn_file_actions_addclose(posix_spawn_file_actions_t *actions, int fd);
  int posix_spawn_file_actions_destroy(posix_spawn_file_actions_t *actions);
]]

local pipefd = ffi.new("int[2]")
local pid_ptr = ffi.new("int[1]")
local status = ffi.new("int[1]")
local buffer = ffi.new("char[4096]")
local path_buf = ffi.new("char[4096]")
local argv = ffi.new("const char*[3]")
local actions = ffi.new("posix_spawn_file_actions_t")

local EINTR = 4

--- Execute a binary directly using posix_spawnp (Zero-Allocation & Forkless).
--- @param cmd_path string The path to the binary to execute.
--- @param json_payload string The JSON payload to pass as the first argument.
--- @param working_dir string|nil The working directory for the child process, if specified.
--- @return string|nil The captured stdout from the binary, or nil on error.
--- @return string|nil The error message, if an error occurred.
local function execute_binary_fast(cmd_path, json_payload, working_dir)
  if ffi.C.pipe(pipefd) < 0 then
    return nil, "Failed to create pipe"
  end

  if ffi.C.posix_spawn_file_actions_init(actions) ~= 0 then
    ffi.C.close(pipefd[0])
    ffi.C.close(pipefd[1])
    return nil, "Failed to init file actions"
  end

  ffi.C.posix_spawn_file_actions_adddup2(actions, pipefd[1], 1) -- redirect stdout vào đầu ghi của pipe
  ffi.C.posix_spawn_file_actions_addclose(actions, pipefd[0])   -- đóng đầu đọc ở tiến trình con
  ffi.C.posix_spawn_file_actions_addclose(actions, pipefd[1])   -- đóng đầu ghi gốc sau khi đã dup2

  argv[0] = cmd_path
  argv[1] = json_payload
  argv[2] = nil

  local old_dir = nil
  if working_dir then
    if ffi.C.getcwd(path_buf, 4096) ~= nil then
      old_dir = ffi.string(path_buf)
    end
    ffi.C.chdir(working_dir)
  end


  local spawn_res = ffi.C.posix_spawnp(pid_ptr, cmd_path, actions, nil, argv, nil)

  if old_dir then
    ffi.C.chdir(old_dir)
  end


  ffi.C.posix_spawn_file_actions_destroy(actions)

  if spawn_res ~= 0 then
    ffi.C.close(pipefd[0])
    ffi.C.close(pipefd[1])
    return nil, "Failed to spawn process: " .. tostring(spawn_res)
  end

  ffi.C.close(pipefd[1])

  local pid = pid_ptr[0]
  local chunks = {}

  while true do
    local bytes_read = ffi.C.read(pipefd[0], buffer, 4096)
    if bytes_read > 0 then
      table.insert(chunks, ffi.string(buffer, bytes_read))
    elseif bytes_read == 0 then
      break
    else
      if ffi.errno() ~= EINTR then
        break
      end
    end
  end
  ffi.C.close(pipefd[0])

  while ffi.C.waitpid(pid, status, 0) < 0 do
    if ffi.errno() ~= EINTR then break end
  end

  return table.concat(chunks)
end

return execute_binary_fast
