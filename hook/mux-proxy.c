/* UU's uuyc-mux session bookkeeping mapped onto native PTY adapters. */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int sibling_path(char *path, size_t capacity, const char *name);

static void log_result(const char *command, const char *session, int result)
{
    char path[32768];
    char line[384];
    HANDLE file;
    DWORD length;
    DWORD written;
    if (!sibling_path(path, sizeof(path), "uur-mux-adapter.status"))
        return;
    file = CreateFileA(path, FILE_APPEND_DATA,
                       FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                       NULL, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE)
        return;
    length = (DWORD)snprintf(line, sizeof(line),
                             "command=%s session=%s result=%d\r\n",
                             command != NULL ? command : "",
                             session != NULL ? session : "", result);
    if (length < sizeof(line))
        WriteFile(file, line, length, &written, NULL);
    CloseHandle(file);
}

static int sibling_path(char *path, size_t capacity, const char *name)
{
    DWORD length = GetModuleFileNameA(NULL, path, (DWORD)capacity);
    char *separator;
    size_t prefix;
    size_t name_length = strlen(name);
    if (length == 0 || length >= capacity)
        return 0;
    separator = strrchr(path, '\\');
    if (separator == NULL)
        return 0;
    prefix = (size_t)(separator - path) + 1;
    if (prefix + name_length + 1 > capacity)
        return 0;
    memcpy(path + prefix, name, name_length + 1);
    return 1;
}

static int valid_session(const char *session)
{
    size_t index;
    if (session == NULL || session[0] == '\0')
        return 0;
    for (index = 0; session[index] != '\0'; ++index) {
        char c = session[index];
        if (index >= 126 || !((c >= 'a' && c <= 'z') ||
                              (c >= 'A' && c <= 'Z') ||
                              (c >= '0' && c <= '9') || c == '-' || c == '_'))
            return 0;
    }
    return 1;
}

static int marker_path(const char *session, char *path, size_t capacity)
{
    char name[192];
    if (!valid_session(session) ||
        snprintf(name, sizeof(name), "uur-terminal-session-%s.active",
                 session) >= (int)sizeof(name))
        return 0;
    return sibling_path(path, capacity, name);
}

static DWORD marker_pid(const char *session, char *path, size_t capacity)
{
    HANDLE file;
    char content[64];
    DWORD count;
    char *end;
    unsigned long pid;
    if (!marker_path(session, path, capacity))
        return 0;
    file = CreateFileA(path, GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE |
                       FILE_SHARE_DELETE, NULL, OPEN_EXISTING,
                       FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE)
        return 0;
    if (!ReadFile(file, content, sizeof(content) - 1, &count, NULL)) {
        CloseHandle(file);
        return 0;
    }
    CloseHandle(file);
    content[count] = '\0';
    pid = strtoul(content, &end, 10);
    if (end == content || pid == 0 || pid > 0xffffffffUL)
        return 0;
    return (DWORD)pid;
}

static int session_alive(const char *session, char *path, size_t capacity)
{
    return marker_pid(session, path, capacity) != 0;
}

static int wait_for_session(const char *session, char *path, size_t capacity)
{
    unsigned attempt;
    for (attempt = 0; attempt < 75; ++attempt) {
        if (session_alive(session, path, capacity))
            return 1;
        Sleep(10);
    }
    return 0;
}

static int attach_session(const char *session, char *path, size_t capacity)
{
    if (!wait_for_session(session, path, capacity))
        return 1;
    log_result("attach-start", session, 0);
    while (marker_pid(session, path, capacity) != 0)
        Sleep(100);
    return 0;
}

static const char *option_value(int argc, char **argv, const char *name)
{
    int index;
    for (index = 1; index + 1 < argc; ++index) {
        if (strcmp(argv[index], name) == 0)
            return argv[index + 1];
    }
    return NULL;
}

static const char *command_name(int argc, char **argv)
{
    int index;
    for (index = 1; index < argc; ++index) {
        if ((strcmp(argv[index], "-L") == 0 ||
             strcmp(argv[index], "-f") == 0 ||
             strcmp(argv[index], "-S") == 0) && index + 1 < argc) {
            ++index;
            continue;
        }
        return argv[index];
    }
    return NULL;
}

static int delegate_version(void)
{
    char executable[32768];
    char command[33000];
    STARTUPINFOA startup;
    PROCESS_INFORMATION process;
    DWORD code = 1;
    if (!sibling_path(executable, sizeof(executable),
                      "uuyc-mux.exe.uur-original"))
        return 1;
    snprintf(command, sizeof(command), "\"%s\" --version", executable);
    ZeroMemory(&startup, sizeof(startup));
    ZeroMemory(&process, sizeof(process));
    startup.cb = sizeof(startup);
    if (!CreateProcessA(executable, command, NULL, NULL, TRUE, 0, NULL,
                        NULL, &startup, &process))
        return 1;
    WaitForSingleObject(process.hProcess, 10000);
    GetExitCodeProcess(process.hProcess, &code);
    CloseHandle(process.hThread);
    CloseHandle(process.hProcess);
    return (int)code;
}

static int list_sessions(void)
{
    char pattern[32768];
    WIN32_FIND_DATAA entry;
    HANDLE search;
    char *separator;
    if (!sibling_path(pattern, sizeof(pattern), "uur-terminal-session-*.active"))
        return 1;
    separator = strrchr(pattern, '\\');
    search = FindFirstFileA(pattern, &entry);
    if (search == INVALID_HANDLE_VALUE)
        return 0;
    do {
        const char *prefix = "uur-terminal-session-";
        const char *suffix = ".active";
        size_t length = strlen(entry.cFileName);
        size_t prefix_length = strlen(prefix);
        size_t suffix_length = strlen(suffix);
        if (length > prefix_length + suffix_length &&
            strncmp(entry.cFileName, prefix, prefix_length) == 0 &&
            strcmp(entry.cFileName + length - suffix_length, suffix) == 0) {
            char session[128];
            char marker[32768];
            size_t session_length = length - prefix_length - suffix_length;
            memcpy(session, entry.cFileName + prefix_length, session_length);
            session[session_length] = '\0';
            if (session_alive(session, marker, sizeof(marker)))
                printf("%s\n", session);
        }
    } while (FindNextFileA(search, &entry));
    FindClose(search);
    (void)separator;
    return 0;
}

int main(int argc, char **argv)
{
    const char *command = command_name(argc, argv);
    const char *session = option_value(argc, argv, "-t");
    char marker[32768];
    int result;
    if (command == NULL || strcmp(command, "--version") == 0 ||
        strcmp(command, "version") == 0) {
        result = delegate_version();
        log_result(command, session, result);
        return result;
    }
    if (strcmp(command, "has-session") == 0 || strcmp(command, "has") == 0) {
        result = wait_for_session(session, marker, sizeof(marker)) ? 0 : 1;
        log_result(command, session, result);
        return result;
    }
    if (strcmp(command, "list-sessions") == 0 || strcmp(command, "ls") == 0) {
        result = list_sessions();
        log_result(command, session, result);
        return result;
    }
    if (strcmp(command, "attach") == 0 ||
        strcmp(command, "attach-session") == 0 ||
        strcmp(command, "a") == 0 || strcmp(command, "at") == 0) {
        result = attach_session(session, marker, sizeof(marker));
        log_result(command, session, result);
        return result;
    }
    if (strcmp(command, "kill-session") == 0 ||
        strcmp(command, "kill-ses") == 0) {
        DWORD pid = marker_pid(session, marker, sizeof(marker));
        if (marker[0] != '\0')
            DeleteFileA(marker);
        result = pid ? 0 : 1;
        log_result(command, session, result);
        return result;
    }
    /* Configuration and display commands are no-ops for a native PTY. */
    log_result(command, session, 0);
    return 0;
}
