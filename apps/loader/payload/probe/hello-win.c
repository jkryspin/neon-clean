/* neon-probe: minimal test DLL - appends one line to C:\Neon\hello.log on load */
#include <windows.h>

BOOL WINAPI DllMain(HINSTANCE hinst, DWORD reason, LPVOID reserved) {
    (void)hinst; (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) {
        HANDLE f = CreateFileA("C:\\Neon\\hello.log",
                               FILE_APPEND_DATA, FILE_SHARE_READ | FILE_SHARE_WRITE,
                               NULL, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
        if (f != INVALID_HANDLE_VALUE) {
            char buf[128];
            DWORD pid = GetCurrentProcessId();
            int n = 0;
            const char *pfx = "hello: loaded into pid ";
            for (const char *c = pfx; *c; c++) buf[n++] = *c;
            /* pid as decimal */
            char digits[12];
            int d = 0;
            if (!pid) digits[d++] = '0';
            while (pid) { digits[d++] = (char)('0' + pid % 10); pid /= 10; }
            while (d) buf[n++] = digits[--d];
            const char *sfx = "\r\n";
            for (const char *c = sfx; *c; c++) buf[n++] = *c;
            DWORD written = 0;
            WriteFile(f, buf, (DWORD)n, &written, NULL);
            CloseHandle(f);
        }
    }
    return TRUE;
}
