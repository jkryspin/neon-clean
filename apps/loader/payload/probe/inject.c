/* neon_inject.exe - load a DLL into a running process using the standard Win32 path.
 *
 *   wine neon_inject.exe <pid-or-image-name> <path-to-dll>
 *   wine neon_inject.exe <pid-or-image-name> --unload <module-name>
 *   wine neon_inject.exe --all <image-name> <path-to-dll>      # every matching process
 *   wine neon_inject.exe --all <image-name> --unload <module>  # every matching process
 *
 * Run it with the SAME WINEPREFIX as the target: each prefix has its own wineserver, and OpenProcess can only
 * see processes of the wineserver it talks to.
 *
 * Why this and not gdb/ptrace: under Wine, OpenProcess/WriteProcessMemory/CreateRemoteThread are serviced by
 * wineserver (memory via /proc/PID/mem). There is no ptrace attach, no thread-stopping, and no TracerPid - the
 * signature that took both test accounts to the login screen twice. The cost is a brief new thread in the
 * target, which exits as soon as LoadLibraryW returns.
 */
#include <windows.h>
#include <tlhelp32.h>
#include <psapi.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>

static DWORD find_pid(const char *s) {
    char *end; unsigned long v = strtoul(s, &end, 10);
    if (*end == 0 && v) return (DWORD)v;
    HANDLE snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    PROCESSENTRY32 pe = { sizeof pe }; DWORD found = 0;
    for (BOOL ok = Process32First(snap, &pe); ok; ok = Process32Next(snap, &pe))
        if (!_stricmp(pe.szExeFile, s)) { found = pe.th32ProcessID; break; }
    CloseHandle(snap); return found;
}

static HANDLE open_pid(DWORD pid) {
    return OpenProcess(PROCESS_CREATE_THREAD | PROCESS_QUERY_INFORMATION | PROCESS_VM_OPERATION |
                       PROCESS_VM_WRITE | PROCESS_VM_READ, FALSE, pid);
}

static int remote_call(HANDLE proc, const char *fn, const wchar_t *arg) {
    HMODULE k32 = GetModuleHandleW(L"kernel32.dll");
    FARPROC target = GetProcAddress(k32, fn);
    if (!target) { fprintf(stderr, "no %s\n", fn); return 1; }
    SIZE_T bytes = (wcslen(arg) + 1) * sizeof(wchar_t), wrote = 0;
    LPVOID mem = VirtualAllocEx(proc, NULL, bytes, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
    if (!mem) { fprintf(stderr, "VirtualAllocEx failed: %lu\n", GetLastError()); return 1; }
    if (!WriteProcessMemory(proc, mem, arg, bytes, &wrote) || wrote != bytes) {
        fprintf(stderr, "WriteProcessMemory failed: %lu\n", GetLastError()); return 1; }
    HANDLE th = CreateRemoteThread(proc, NULL, 0, (LPTHREAD_START_ROUTINE)target, mem, 0, NULL);
    if (!th) { fprintf(stderr, "CreateRemoteThread failed: %lu\n", GetLastError()); return 1; }
    DWORD w = WaitForSingleObject(th, 10000);
    DWORD code = 0; GetExitCodeThread(th, &code);
    CloseHandle(th);
    VirtualFreeEx(proc, mem, 0, MEM_RELEASE);
    printf("%s: wait=%lu return=%#lx\n", fn, w, code);
    return w == WAIT_OBJECT_0 ? 0 : 1;
}

static int do_load(DWORD pid, const wchar_t *dll) {
    HANDLE proc = open_pid(pid);
    if (!proc) { fprintf(stderr, "OpenProcess(%lu) failed: %lu\n", pid, GetLastError()); return 1; }
    printf("target pid %lu\n", pid);
    int r = remote_call(proc, "LoadLibraryW", dll);
    CloseHandle(proc);
    return r;
}

static int do_unload(DWORD pid, const wchar_t *module) {
    HANDLE proc = open_pid(pid);
    if (!proc) { fprintf(stderr, "OpenProcess(%lu) failed: %lu\n", pid, GetLastError()); return 1; }
    printf("target pid %lu\n", pid);
    HMODULE mods[1024]; DWORD need = 0;
    if (!EnumProcessModules(proc, mods, sizeof mods, &need)) { fprintf(stderr, "EnumProcessModules failed\n"); return 1; }
    for (DWORD i = 0; i < need / sizeof(HMODULE); i++) {
        wchar_t nm[MAX_PATH];
        if (GetModuleBaseNameW(proc, mods[i], nm, MAX_PATH) && !_wcsicmp(nm, module)) {
            HANDLE th = CreateRemoteThread(proc, NULL, 0, (LPTHREAD_START_ROUTINE)GetProcAddress(GetModuleHandleW(L"kernel32.dll"), "FreeLibrary"), mods[i], 0, NULL);
            if (!th) { fprintf(stderr, "unload thread failed: %lu\n", GetLastError()); return 1; }
            WaitForSingleObject(th, 10000); CloseHandle(th); printf("unloaded %ls\n", module); return 0;
        }
    }
    fprintf(stderr, "module %ls not found in target\n", module); return 1;
}

/* Inject into every process with the exact image name. */
static int for_all(const char *name, const wchar_t *arg, int unload) {
    HANDLE snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    if (snap == INVALID_HANDLE_VALUE) { fprintf(stderr, "snapshot failed\n"); return 1; }
    PROCESSENTRY32 pe = { sizeof pe };
    int n = 0, rc = 0;
    for (BOOL ok = Process32First(snap, &pe); ok; ok = Process32Next(snap, &pe))
        if (!_stricmp(pe.szExeFile, name)) { rc |= unload ? do_unload(pe.th32ProcessID, arg) : do_load(pe.th32ProcessID, arg); n++; }
    CloseHandle(snap);
    if (!n) { fprintf(stderr, "no process named %s\n", name); return 1; }
    printf("%s into %d %s process(es)\n", unload ? "unloaded" : "injected", n, name);
    return rc;
}

int main(int argc, char **argv) {
    if (argc >= 5 && !strcmp(argv[1], "--all") && !strcmp(argv[3], "--unload")) {
        wchar_t arg[MAX_PATH];
        MultiByteToWideChar(CP_ACP, 0, argv[4], -1, arg, MAX_PATH);
        return for_all(argv[2], arg, 1);
    }
    if (argc >= 4 && !strcmp(argv[1], "--all")) {
        wchar_t arg[MAX_PATH];
        MultiByteToWideChar(CP_ACP, 0, argv[3], -1, arg, MAX_PATH);
        return for_all(argv[2], arg, 0);
    }
    if (argc < 3) { fprintf(stderr, "usage: %s <pid|name> <dll> | %s <pid|name> --unload <module> | %s --all <name> <dll|--unload module>\n", argv[0], argv[0], argv[0]); return 2; }
    DWORD pid = find_pid(argv[1]);
    if (!pid) { fprintf(stderr, "no such process: %s\n", argv[1]); return 1; }
    wchar_t arg[MAX_PATH];
    if (!strcmp(argv[2], "--unload")) {
        if (argc < 4) return 2;
        MultiByteToWideChar(CP_ACP, 0, argv[3], -1, arg, MAX_PATH);
        return do_unload(pid, arg);
    }
    MultiByteToWideChar(CP_ACP, 0, argv[2], -1, arg, MAX_PATH);
    return do_load(pid, arg);
}
