#define WIN32_LEAN_AND_MEAN
#include <winsock2.h>
#include <windows.h>

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "terminal-protocol.h"

static SOCKET bridge = INVALID_SOCKET;
static CRITICAL_SECTION write_lock;
static HANDLE stopping;
static HANDLE terminal_input;
static HANDLE terminal_output;
static HANDLE terminal_control;
static volatile LONG saw_input;
static volatile LONG saw_output;

static int executable_sibling(char *path, size_t capacity, const char *name);

static int delegate_visible_attach(void)
{
    char executable[32768];
    char *command;
    STARTUPINFOA startup;
    PROCESS_INFORMATION process;
    DWORD exit_code = 1;
    DWORD wait;
    if (!executable_sibling(executable, sizeof(executable),
                            "conpty_bridge.exe.uur-original"))
        return 1;
    command = _strdup(GetCommandLineA());
    if (command == NULL)
        return 1;
    ZeroMemory(&startup, sizeof(startup));
    ZeroMemory(&process, sizeof(process));
    startup.cb = sizeof(startup);
    if (!CreateProcessA(executable, command, NULL, NULL, FALSE,
                        CREATE_NO_WINDOW, NULL, NULL, &startup, &process)) {
        free(command);
        return 1;
    }
    free(command);
    CloseHandle(process.hThread);
    wait = WaitForSingleObject(process.hProcess, 15000);
    if (wait == WAIT_OBJECT_0)
        GetExitCodeProcess(process.hProcess, &exit_code);
    else
        TerminateProcess(process.hProcess, 1);
    CloseHandle(process.hProcess);
    return (int)exit_code;
}

struct launch_options {
    HANDLE input;
    HANDLE output;
    HANDLE control;
    uint16_t columns;
    uint16_t rows;
    int explicit_handles;
    int attach_existing;
    int visible_attach;
    char session[128];
};

static void write_status(const char *stage, DWORD detail)
{
    char path[32768];
    char line[192];
    DWORD length;
    DWORD written;
    HANDLE file;

    if (!executable_sibling(path, sizeof(path), "uur-terminal-adapter.status"))
        return;
    file = CreateFileA(path, FILE_APPEND_DATA,
                       FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                       NULL, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE)
        return;
    length = (DWORD)snprintf(line, sizeof(line),
                             "pid=%lu stage=%s detail=%lu\r\n",
                             (unsigned long)GetCurrentProcessId(), stage,
                             (unsigned long)detail);
    if (length < sizeof(line))
        WriteFile(file, line, length, &written, NULL);
    CloseHandle(file);
    SecureZeroMemory(path, sizeof(path));
}

static int send_all(const void *data, size_t length)
{
    const char *cursor = data;
    while (length > 0) {
        int request = length > INT_MAX ? INT_MAX : (int)length;
        int written = send(bridge, cursor, request, 0);
        if (written <= 0)
            return 0;
        cursor += written;
        length -= (size_t)written;
    }
    return 1;
}

static int receive_all(void *data, size_t length)
{
    char *cursor = data;
    while (length > 0) {
        int request = length > INT_MAX ? INT_MAX : (int)length;
        int received = recv(bridge, cursor, request, 0);
        if (received <= 0)
            return 0;
        cursor += received;
        length -= (size_t)received;
    }
    return 1;
}

static int send_frame(uint8_t type, const void *payload, uint32_t length)
{
    struct uur_terminal_frame frame;
    int result;

    ZeroMemory(&frame, sizeof(frame));
    frame.type = type;
    frame.length = htonl(length);
    EnterCriticalSection(&write_lock);
    result = send_all(&frame, sizeof(frame));
    if (result && length != 0)
        result = send_all(payload, length);
    LeaveCriticalSection(&write_lock);
    return result;
}

static int executable_sibling(char *path, size_t capacity, const char *name)
{
    DWORD length = GetModuleFileNameA(NULL, path, (DWORD)capacity);
    char *separator;
    size_t prefix;
    size_t name_length = strlen(name);

    if (length == 0 || length >= capacity)
        return 0;
    separator = strrchr(path, '\\');
    if (separator == NULL)
        separator = strrchr(path, '/');
    if (separator == NULL)
        return 0;
    prefix = (size_t)(separator - path) + 1;
    if (prefix + name_length + 1 > capacity)
        return 0;
    memcpy(path + prefix, name, name_length + 1);
    return 1;
}

static int valid_token(const char *token)
{
    size_t index;
    if (strlen(token) != UUR_TERMINAL_TOKEN_BYTES)
        return 0;
    for (index = 0; index < UUR_TERMINAL_TOKEN_BYTES; ++index) {
        if (!((token[index] >= '0' && token[index] <= '9') ||
              (token[index] >= 'a' && token[index] <= 'f')))
            return 0;
    }
    return 1;
}

static int load_config(uint16_t *port, char *token)
{
    char path[32768];
    char content[256];
    DWORD count;
    LARGE_INTEGER size;
    HANDLE file = INVALID_HANDLE_VALUE;
    unsigned parsed_port;
    char parsed_token[UUR_TERMINAL_TOKEN_BYTES + 1];
    int result = 0;

    if (!executable_sibling(path, sizeof(path), UUR_TERMINAL_CONFIG))
        goto done;
    file = CreateFileA(path, GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_DELETE,
                       NULL, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE || !GetFileSizeEx(file, &size) ||
        size.QuadPart <= 0 || size.QuadPart >= (LONGLONG)sizeof(content) ||
        !ReadFile(file, content, (DWORD)size.QuadPart, &count, NULL) ||
        count != (DWORD)size.QuadPart)
        goto done;
    content[count] = '\0';
    if (sscanf(content, "version=1\nport=%u\ntoken=%64[0-9a-f]\n",
               &parsed_port, parsed_token) != 2 || parsed_port == 0 ||
        parsed_port > 65535 || !valid_token(parsed_token))
        goto done;
    *port = (uint16_t)parsed_port;
    memcpy(token, parsed_token, UUR_TERMINAL_TOKEN_BYTES + 1);
    result = 1;

done:
    if (file != INVALID_HANDLE_VALUE)
        CloseHandle(file);
    SecureZeroMemory(content, sizeof(content));
    SecureZeroMemory(parsed_token, sizeof(parsed_token));
    SecureZeroMemory(path, sizeof(path));
    return result;
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

static int has_option(int argc, char **argv, const char *name)
{
    int index;
    for (index = 1; index < argc; ++index) {
        if (strcmp(argv[index], name) == 0)
            return 1;
    }
    return 0;
}

static HANDLE parse_handle(const char *value)
{
    unsigned long long raw;
    char *end;

    if (value == NULL)
        return INVALID_HANDLE_VALUE;
    raw = _strtoui64(value, &end, 0);
    if (*value == '\0' || *end != '\0' || raw == 0)
        return INVALID_HANDLE_VALUE;
    return (HANDLE)(uintptr_t)raw;
}

static uint16_t parse_dimension(const char *value, uint16_t fallback,
                                uint16_t minimum, uint16_t maximum)
{
    unsigned long parsed;
    char *end;

    if (value == NULL)
        return fallback;
    parsed = strtoul(value, &end, 10);
    if (*value == '\0' || *end != '\0' || parsed < minimum || parsed > maximum)
        return fallback;
    return (uint16_t)parsed;
}

static int parse_launch_options(int argc, char **argv,
                                struct launch_options *options)
{
    const char *input = option_value(argc, argv, "--stdin-handle");
    const char *output = option_value(argc, argv, "--stdout-handle");

    ZeroMemory(options, sizeof(*options));
    options->columns = parse_dimension(option_value(argc, argv, "--cols"),
                                       80, 20, 1000);
    options->rows = parse_dimension(option_value(argc, argv, "--rows"),
                                    24, 5, 500);
    options->control = parse_handle(option_value(argc, argv, "--ctl-handle"));
    options->attach_existing =
        has_option(argc, argv, "--uuyc-mux-attach-existing");
    options->visible_attach = has_option(argc, argv, "--visible-attach");
    {
        const char *session = option_value(argc, argv, "--uuyc-mux-session");
        size_t index;
        if (session != NULL) {
            for (index = 0; session[index] != '\0'; ++index) {
                char c = session[index];
                if (index + 1 >= sizeof(options->session) ||
                    !((c >= 'a' && c <= 'z') ||
                      (c >= 'A' && c <= 'Z') ||
                      (c >= '0' && c <= '9') || c == '-' || c == '_'))
                    return 0;
                options->session[index] = c;
            }
        }
    }
    if (input == NULL && output == NULL) {
        options->input = GetStdHandle(STD_INPUT_HANDLE);
        options->output = GetStdHandle(STD_OUTPUT_HANDLE);
        return options->input != INVALID_HANDLE_VALUE &&
               options->output != INVALID_HANDLE_VALUE;
    }
    options->input = parse_handle(input);
    options->output = parse_handle(output);
    options->explicit_handles = 1;
    return options->input != INVALID_HANDLE_VALUE &&
           options->output != INVALID_HANDLE_VALUE;
}

static int session_marker_path(const char *session, char *path, size_t capacity)
{
    char name[192];
    if (session == NULL || session[0] == '\0')
        return 0;
    if (snprintf(name, sizeof(name), "uur-terminal-session-%s.active",
                 session) >= (int)sizeof(name))
        return 0;
    return executable_sibling(path, capacity, name);
}

static int publish_session(const char *session, char *path, size_t capacity)
{
    HANDLE file;
    char content[64];
    DWORD length;
    DWORD written;
    if (!session_marker_path(session, path, capacity))
        return session == NULL || session[0] == '\0';
    file = CreateFileA(path, GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_DELETE,
                       NULL, CREATE_ALWAYS, FILE_ATTRIBUTE_HIDDEN, NULL);
    if (file == INVALID_HANDLE_VALUE)
        return 0;
    length = (DWORD)snprintf(content, sizeof(content), "%lu\r\n",
                             (unsigned long)GetCurrentProcessId());
    if (!WriteFile(file, content, length, &written, NULL) || written != length) {
        CloseHandle(file);
        DeleteFileA(path);
        return 0;
    }
    CloseHandle(file);
    return 1;
}

static void console_size(uint16_t *columns, uint16_t *rows)
{
    CONSOLE_SCREEN_BUFFER_INFO info;
    *columns = 80;
    *rows = 24;
    if (GetConsoleScreenBufferInfo(terminal_output, &info)) {
        SHORT width = info.srWindow.Right - info.srWindow.Left + 1;
        SHORT height = info.srWindow.Bottom - info.srWindow.Top + 1;
        if (width > 0)
            *columns = (uint16_t)width;
        if (height > 0)
            *rows = (uint16_t)height;
    }
}

static DWORD WINAPI stdin_worker(LPVOID unused)
{
    unsigned char data[16384];
    DWORD count;
    (void)unused;

    while (WaitForSingleObject(stopping, 0) == WAIT_TIMEOUT) {
        if (!ReadFile(terminal_input, data, sizeof(data), &count, NULL) || count == 0) {
            send_frame(UUR_TERMINAL_EOF, NULL, 0);
            break;
        }
        if (InterlockedCompareExchange(&saw_input, 1, 0) == 0)
            write_status("first-input", count);
        if (!send_frame(UUR_TERMINAL_DATA, data, count))
            break;
    }
    return 0;
}

static DWORD WINAPI resize_worker(LPVOID unused)
{
    uint16_t previous_columns = 0;
    uint16_t previous_rows = 0;
    (void)unused;

    while (WaitForSingleObject(stopping, 250) == WAIT_TIMEOUT) {
        uint16_t columns;
        uint16_t rows;
        uint16_t dimensions[2];
        console_size(&columns, &rows);
        if (columns == previous_columns && rows == previous_rows)
            continue;
        previous_columns = columns;
        previous_rows = rows;
        dimensions[0] = htons(columns);
        dimensions[1] = htons(rows);
        if (!send_frame(UUR_TERMINAL_RESIZE, dimensions, sizeof(dimensions)))
            break;
    }
    return 0;
}

static DWORD WINAPI control_worker(LPVOID unused)
{
    unsigned char type;
    DWORD count;
    (void)unused;
    while (WaitForSingleObject(stopping, 0) == WAIT_TIMEOUT) {
        if (!ReadFile(terminal_control, &type, 1, &count, NULL) || count != 1)
            break;
        if (type == 1) {
            uint16_t dimensions[2];
            uint16_t payload[2];
            if (!ReadFile(terminal_control, dimensions, sizeof(dimensions),
                          &count, NULL) || count != sizeof(dimensions))
                break;
            payload[0] = htons(dimensions[0]);
            payload[1] = htons(dimensions[1]);
            if (!send_frame(UUR_TERMINAL_RESIZE, payload, sizeof(payload)))
                break;
            write_status("resize", ((DWORD)dimensions[0] << 16) | dimensions[1]);
        } else if (type == 2) {
            write_status("control-close", 0);
            SetEvent(stopping);
            shutdown(bridge, SD_BOTH);
            break;
        }
    }
    return 0;
}

int main(int argc, char **argv)
{
    WSADATA winsock;
    struct sockaddr_in address;
    struct uur_terminal_hello hello;
    char token[UUR_TERMINAL_TOKEN_BYTES + 1];
    uint16_t port;
    uint16_t columns;
    uint16_t rows;
    unsigned char accepted;
    unsigned char output_buffer[16384];
    HANDLE input_thread = NULL;
    HANDLE size_thread = NULL;
    HANDLE control_thread = NULL;
    DWORD written;
    int received;
    int exit_code = 1;
    struct launch_options options;
    char session_marker[32768] = "";

    write_status("started", 0);
    if (!parse_launch_options(argc, argv, &options)) {
        write_status("invalid-arguments", GetLastError());
        return 1;
    }
    terminal_input = options.input;
    terminal_output = options.output;
    terminal_control = options.control;
    if (options.visible_attach) {
        write_status("mode-visible", 0);
        return delegate_visible_attach();
    }
    write_status(options.attach_existing ? "mode-attach" : "mode-new", 0);
    if (!load_config(&port, token)) {
        write_status("config-unavailable", GetLastError());
        return 2;
    }
    if (WSAStartup(MAKEWORD(2, 2), &winsock) != 0) {
        write_status("winsock-unavailable", WSAGetLastError());
        return 3;
    }
    if (!options.attach_existing &&
        !publish_session(options.session, session_marker,
                         sizeof(session_marker))) {
        write_status("session-publish-failed", GetLastError());
        WSACleanup();
        return 4;
    }
    bridge = socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
    if (bridge == INVALID_SOCKET)
        goto done;
    ZeroMemory(&address, sizeof(address));
    address.sin_family = AF_INET;
    address.sin_port = htons(port);
    address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    if (connect(bridge, (struct sockaddr *)&address, sizeof(address)) != 0) {
        write_status("connect-failed", WSAGetLastError());
        goto done;
    }

    if (options.explicit_handles) {
        columns = options.columns;
        rows = options.rows;
    } else {
        console_size(&columns, &rows);
    }
    ZeroMemory(&hello, sizeof(hello));
    hello.magic = htonl(UUR_TERMINAL_MAGIC);
    hello.version = htons(UUR_TERMINAL_VERSION);
    hello.token_length = htons(UUR_TERMINAL_TOKEN_BYTES);
    hello.columns = htons(columns);
    hello.rows = htons(rows);
    if (!send_all(&hello, sizeof(hello)) ||
        !send_all(token, UUR_TERMINAL_TOKEN_BYTES) ||
        !receive_all(&accepted, 1) || accepted != UUR_TERMINAL_ACCEPTED)
        {
            write_status("handshake-failed", WSAGetLastError());
            goto done;
        }

    write_status("running", 0);

    InitializeCriticalSection(&write_lock);
    stopping = CreateEventW(NULL, TRUE, FALSE, NULL);
    if (stopping == NULL) {
        DeleteCriticalSection(&write_lock);
        goto done;
    }
    input_thread = CreateThread(NULL, 0, stdin_worker, NULL, 0, NULL);
    if (options.explicit_handles && terminal_control != INVALID_HANDLE_VALUE)
        control_thread = CreateThread(NULL, 0, control_worker, NULL, 0, NULL);
    else if (!options.explicit_handles)
        size_thread = CreateThread(NULL, 0, resize_worker, NULL, 0, NULL);
    if (input_thread == NULL ||
        (options.explicit_handles && terminal_control != INVALID_HANDLE_VALUE &&
         control_thread == NULL) ||
        (!options.explicit_handles && size_thread == NULL))
        goto workers_done;

    while ((received = recv(bridge, (char *)output_buffer,
                            sizeof(output_buffer), 0)) > 0) {
        if (InterlockedCompareExchange(&saw_output, 1, 0) == 0)
            write_status("first-output", (DWORD)received);
        if (!WriteFile(terminal_output, output_buffer,
                       (DWORD)received, &written, NULL)) {
            write_status("output-write-failed", GetLastError());
            break;
        }
    }
    exit_code = 0;

workers_done:
    SetEvent(stopping);
    shutdown(bridge, SD_BOTH);
    if (input_thread != NULL) {
        CancelSynchronousIo(input_thread);
        WaitForSingleObject(input_thread, 1000);
        CloseHandle(input_thread);
    }
    if (size_thread != NULL) {
        WaitForSingleObject(size_thread, 1000);
        CloseHandle(size_thread);
    }
    if (control_thread != NULL) {
        CancelSynchronousIo(control_thread);
        WaitForSingleObject(control_thread, 1000);
        CloseHandle(control_thread);
    }
    CloseHandle(stopping);
    DeleteCriticalSection(&write_lock);

done:
    if (session_marker[0] != '\0')
        DeleteFileA(session_marker);
    if (bridge != INVALID_SOCKET)
        closesocket(bridge);
    SecureZeroMemory(token, sizeof(token));
    WSACleanup();
    write_status("exited", (DWORD)exit_code);
    return exit_code;
}
