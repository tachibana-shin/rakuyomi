local ffi = require("ffi")
local closeInheritedFDs = require("utils/closeInheritedFDs")

ffi.cdef [[
  int pipe(int pipefd[2]);
  int fork(void);
  int dup2(int oldfd, int newfd);
  int execvp(const char *file, char *const argv[]);
  int close(int fd);
  ptrdiff_t read(int fd, void *buf, size_t count);
  int waitpid(int pid, int *wstatus, int options);
  int chdir(const char *path);
  void _exit(int status);
]]

--- Execute a binary directly using fork/execvp.
---
--- This is significantly faster and more reliable than using os.execute or KOReader's
--- subprocess mechanisms, as it avoids the overhead of starting a shell and the
--- potential security issues with raw string concatenation.
---
--- @param cmd_path string The path to the binary to execute.
--- @param json_payload string The JSON payload to pass as the first argument.
--- @param working_dir string|nil The working directory for the child process, if specified.
--- @return string|nil The captured stdout from the binary, or nil on error.
--- @return string|nil The error message, if an error occurred.
local function execute_binary_fast(cmd_path, json_payload, working_dir)
  local pipefd = ffi.new("int[2]")
  if ffi.C.pipe(pipefd) < 0 then
    return nil, "Failed to create pipe"
  end

  local argv = ffi.new("const char*[3]", { cmd_path, json_payload, nil })

  local pid = ffi.C.fork()
  if pid < 0 then
    ffi.C.close(pipefd[0])
    ffi.C.close(pipefd[1])
    return nil, "Failed to fork process"
  elseif pid == 0 then
    ffi.C.close(pipefd[0])
    ffi.C.dup2(pipefd[1], 1)
    ffi.C.close(pipefd[1])

    closeInheritedFDs(3)

    if working_dir then
      ffi.C.chdir(working_dir)
    end

    ffi.C.execvp(cmd_path, ffi.cast("char *const *", argv))

    ffi.C._exit(127)
  else
    ffi.C.close(pipefd[1])

    local chunks = {}
    local buffer = ffi.new("char[4096]")
    local EINTR = 4

    while true do
      local bytes_read = ffi.C.read(pipefd[0], buffer, 4096)
      if bytes_read > 0 then
        table.insert(chunks, ffi.string(buffer, bytes_read))
      elseif bytes_read == 0 then
        break
      else
        local err = ffi.errno()
        if err ~= EINTR then
          break
        end
      end
    end
    ffi.C.close(pipefd[0])

    local status = ffi.new("int[1]")
    while ffi.C.waitpid(pid, status, 0) < 0 do
      if ffi.errno() ~= EINTR then break end
    end

    return table.concat(chunks)
  end
end

return execute_binary_fast
