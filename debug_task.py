import ctypes, ctypes.util, os, subprocess

# Load libSystem
lib = ctypes.CDLL('/usr/lib/libSystem.B.dylib')

# Get errno
libc = ctypes.CDLL('libc.dylib')
errno = ctypes.cast(libc.errno, ctypes.POINTER(ctypes.c_int))

# task_for_pid
lib.task_for_pid.argtypes = [ctypes.c_uint32, ctypes.c_int, ctypes.POINTER(ctypes.c_uint32)]
lib.task_for_pid.restype = ctypes.c_int

# mach_task_self
mach_self = ctypes.c_uint32.in_dll(lib, 'mach_task_self')
print(f'mach_task_self = {mach_self.value}')

# Try our own PID
my_pid = os.getpid()
task_port = ctypes.c_uint32()
ret = lib.task_for_pid(mach_self.value, my_pid, ctypes.byref(task_port))
print(f'task_for_pid({my_pid}) = {ret}, errno = {errno.contents.value}')

# Try wineserver PID
result = subprocess.run(['pgrep', '-f', 'wineserver'], capture_output=True, text=True)
ws_pid = int(result.stdout.strip().split('\n')[0]) if result.stdout.strip() else None
print(f'wineserver PID = {ws_pid}')

if ws_pid:
    task_port2 = ctypes.c_uint32()
    ret2 = lib.task_for_pid(mach_self.value, ws_pid, ctypes.byref(task_port2))
    print(f'task_for_pid(wineserver={ws_pid}) = {ret2}, errno = {errno.contents.value}')
    if ret2 == 0:
        print(f'  SUCCESS! port = {task_port2.value}')

# Also try PT_ATTACHEXC
PT_ATTACHEXC = 16
PT_ATTACH = 15
lib.ptrace.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int]
lib.ptrace.restype = ctypes.c_int

if ws_pid:
    ret3 = lib.ptrace(PT_ATTACHEXC, ws_pid, 0, 0)
    print(f'ptrace(PT_ATTACHEXC, {ws_pid}) = {ret3}, errno = {errno.contents.value}')
    ret4 = lib.ptrace(PT_ATTACH, ws_pid, 0, 0)
    print(f'ptrace(PT_ATTACH, {ws_pid}) = {ret4}, errno = {errno.contents.value}')
