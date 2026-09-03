/* Minimal WTS provider for UU Remote under Wine's single interactive login. */
#define WIN32_LEAN_AND_MEAN
#define _WIN32_WINNT 0x0A00
#include <windows.h>
#include <wtsapi32.h>

#include <string.h>

/* Ubuntu 22.04 ships MinGW-w64 headers that predate the Windows 8 WTS
 * additions used by current UU builds. Keep the provider ABI self-contained
 * instead of making the package depend on a particular MinGW header release. */
#define UUR_WTS_SESSION_INFO_EX ((WTS_INFO_CLASS)25)
#define UUR_WTS_IS_REMOTE_SESSION ((WTS_INFO_CLASS)29)
#define UUR_WTS_SESSIONSTATE_UNLOCK 0x00000001L

typedef struct uur_wtsinfoex_level1_w {
    ULONG SessionId;
    WTS_CONNECTSTATE_CLASS SessionState;
    LONG SessionFlags;
    WCHAR WinStationName[WINSTATIONNAME_LENGTH + 1];
    WCHAR UserName[USERNAME_LENGTH + 1];
    WCHAR DomainName[DOMAIN_LENGTH + 1];
    LARGE_INTEGER LogonTime;
    LARGE_INTEGER ConnectTime;
    LARGE_INTEGER DisconnectTime;
    LARGE_INTEGER LastInputTime;
    LARGE_INTEGER CurrentTime;
    DWORD IncomingBytes;
    DWORD OutgoingBytes;
    DWORD IncomingFrames;
    DWORD OutgoingFrames;
    DWORD IncomingCompressedBytes;
    DWORD OutgoingCompressedBytes;
} UUR_WTSINFOEX_LEVEL1_W;

typedef struct uur_wtsinfoex_w {
    DWORD Level;
    union {
        UUR_WTSINFOEX_LEVEL1_W WTSInfoExLevel1;
    } Data;
} UUR_WTSINFOEXW, *PUUR_WTSINFOEXW;

_Static_assert(sizeof(UUR_WTSINFOEXW) == 232,
               "WTSINFOEXW x64 ABI layout changed");

static DWORD active_session(void)
{
    DWORD session = WTSGetActiveConsoleSessionId();
    return session == 0xffffffff ? 1 : session;
}

static DWORD normalized_session(DWORD session)
{
    return session == WTS_CURRENT_SESSION ? active_session() : session;
}

static void *allocate(DWORD size)
{
    return HeapAlloc(GetProcessHeap(), HEAP_ZERO_MEMORY, size);
}

static BOOL string_result(LPCWSTR value, LPWSTR *buffer, DWORD *bytes)
{
    DWORD size;
    LPWSTR copy;
    if (buffer == NULL || bytes == NULL) {
        SetLastError(ERROR_INVALID_PARAMETER);
        return FALSE;
    }
    size = ((DWORD)lstrlenW(value) + 1) * sizeof(WCHAR);
    copy = allocate(size);
    if (copy == NULL) {
        SetLastError(ERROR_NOT_ENOUGH_MEMORY);
        return FALSE;
    }
    memcpy(copy, value, size);
    *buffer = copy;
    *bytes = size;
    return TRUE;
}

__declspec(dllexport) BOOL WINAPI WTSEnumerateSessionsW(
    HANDLE server, DWORD reserved, DWORD version,
    PWTS_SESSION_INFOW *sessions, DWORD *count)
{
    static const WCHAR station[] = L"Console";
    DWORD size;
    PWTS_SESSION_INFOW result;
    LPWSTR name;
    (void)server;
    (void)reserved;
    (void)version;
    if (sessions == NULL || count == NULL) {
        SetLastError(ERROR_INVALID_PARAMETER);
        return FALSE;
    }
    size = sizeof(*result) + sizeof(station);
    result = allocate(size);
    if (result == NULL) {
        SetLastError(ERROR_NOT_ENOUGH_MEMORY);
        return FALSE;
    }
    name = (LPWSTR)(result + 1);
    memcpy(name, station, sizeof(station));
    result->SessionId = active_session();
    result->pWinStationName = name;
    result->State = WTSActive;
    *sessions = result;
    *count = 1;
    return TRUE;
}

__declspec(dllexport) BOOL WINAPI WTSQuerySessionInformationW(
    HANDLE server, DWORD session, WTS_INFO_CLASS info_class,
    LPWSTR *buffer, DWORD *bytes)
{
    WCHAR value[256];
    DWORD size;
    (void)server;
    if (buffer == NULL || bytes == NULL) {
        SetLastError(ERROR_INVALID_PARAMETER);
        return FALSE;
    }
    session = normalized_session(session);
    switch ((int)info_class) {
    case WTSSessionId: {
        DWORD *result = allocate(sizeof(*result));
        if (result == NULL)
            break;
        *result = session;
        *buffer = (LPWSTR)result;
        *bytes = sizeof(*result);
        return TRUE;
    }
    case WTSUserName:
        size = sizeof(value) / sizeof(value[0]);
        if (GetUserNameW(value, &size))
            return string_result(value, buffer, bytes);
        break;
    case WTSWinStationName:
        return string_result(L"Console", buffer, bytes);
    case WTSDomainName:
        size = sizeof(value) / sizeof(value[0]);
        if (GetComputerNameW(value, &size))
            return string_result(value, buffer, bytes);
        break;
    case WTSConnectState: {
        WTS_CONNECTSTATE_CLASS *result = allocate(sizeof(*result));
        if (result == NULL)
            break;
        *result = WTSActive;
        *buffer = (LPWSTR)result;
        *bytes = sizeof(*result);
        return TRUE;
    }
    case WTSClientProtocolType: {
        USHORT *result = allocate(sizeof(*result));
        if (result == NULL)
            break;
        *result = 0;
        *buffer = (LPWSTR)result;
        *bytes = sizeof(*result);
        return TRUE;
    }
    case UUR_WTS_SESSION_INFO_EX: {
        PUUR_WTSINFOEXW result = allocate(sizeof(*result));
        FILETIME now;
        DWORD length;
        if (result == NULL)
            break;
        result->Level = 1;
        result->Data.WTSInfoExLevel1.SessionId = session;
        result->Data.WTSInfoExLevel1.SessionState = WTSActive;
        result->Data.WTSInfoExLevel1.SessionFlags =
            UUR_WTS_SESSIONSTATE_UNLOCK;
        lstrcpynW(result->Data.WTSInfoExLevel1.WinStationName, L"Console",
                  WINSTATIONNAME_LENGTH + 1);
        length = USERNAME_LENGTH + 1;
        GetUserNameW(result->Data.WTSInfoExLevel1.UserName, &length);
        length = DOMAIN_LENGTH + 1;
        GetComputerNameW(result->Data.WTSInfoExLevel1.DomainName, &length);
        GetSystemTimeAsFileTime(&now);
        result->Data.WTSInfoExLevel1.CurrentTime.LowPart = now.dwLowDateTime;
        result->Data.WTSInfoExLevel1.CurrentTime.HighPart = now.dwHighDateTime;
        *buffer = (LPWSTR)result;
        *bytes = sizeof(*result);
        return TRUE;
    }
    case UUR_WTS_IS_REMOTE_SESSION: {
        BOOL *result = allocate(sizeof(*result));
        if (result == NULL)
            break;
        *result = FALSE;
        *buffer = (LPWSTR)result;
        *bytes = sizeof(*result);
        return TRUE;
    }
    default:
        SetLastError(ERROR_NOT_SUPPORTED);
        return FALSE;
    }
    SetLastError(ERROR_NOT_ENOUGH_MEMORY);
    return FALSE;
}

__declspec(dllexport) BOOL WINAPI WTSQueryUserToken(ULONG session, PHANDLE token)
{
    HANDLE current = NULL;
    (void)session;
    if (token == NULL) {
        SetLastError(ERROR_INVALID_PARAMETER);
        return FALSE;
    }
    if (!OpenProcessToken(GetCurrentProcess(),
                          TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY,
                          &current))
        return FALSE;
    if (!DuplicateTokenEx(current, MAXIMUM_ALLOWED, NULL,
                          SecurityImpersonation, TokenPrimary, token)) {
        CloseHandle(current);
        return FALSE;
    }
    CloseHandle(current);
    return TRUE;
}

__declspec(dllexport) VOID WINAPI WTSFreeMemory(PVOID memory)
{
    if (memory != NULL)
        HeapFree(GetProcessHeap(), 0, memory);
}

__declspec(dllexport) BOOL WINAPI WTSRegisterSessionNotification(HWND window, DWORD flags)
{
    (void)window;
    (void)flags;
    return TRUE;
}

__declspec(dllexport) BOOL WINAPI WTSUnRegisterSessionNotification(HWND window)
{
    (void)window;
    return TRUE;
}

__declspec(dllexport) BOOL WINAPI WTSConnectSessionW(
    ULONG logon_id, ULONG target_logon_id, PWSTR password, BOOL wait)
{
    (void)logon_id;
    (void)target_logon_id;
    (void)password;
    (void)wait;
    return TRUE;
}

BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, LPVOID reserved)
{
    (void)instance;
    (void)reason;
    (void)reserved;
    return TRUE;
}
