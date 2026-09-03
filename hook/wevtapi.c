/*
 * wevtapi.dll replacement for the managed UU Remote prefix.
 *
 * Wine's builtin wevtapi aborts callers of EvtOpenPublisherMetadata, which
 * UU Remote issues periodically.  This replacement returns the ordinary
 * Windows failure shape for the whole Evt* surface and — critically — links
 * against uur-hook.dll so the Windows loader pulls the input hook into
 * GameViewerServer.exe at process start, before any code runs.  No disk
 * patching, no injector race.
 *
 * Drop this file next to GameViewerServer.exe inside the managed prefix;
 * Wine resolves DLLs from the application directory first.
 *
 * mingw ships no <wevtapi.h>, so the EVT types are defined locally.
 */

#include <windows.h>

typedef PVOID EHANDLE;
typedef EHANDLE EVT_HANDLE;
typedef BOOL(CALLBACK *EVT_SUBSCRIBE_CALLBACK)(PVOID, DWORD, PVOID);

/* Force the uur-hook.dll dependency.  Referencing its marker symbol makes
 * the linker emit the import, which is how the hook enters the client
 * before any client code runs. */
void uur_hook_version(void);
__attribute__((used)) static void (*const uur_hook_anchor)(void) =
    uur_hook_version;

#ifndef ERROR_EVT_PUBLISHER_METADATA_NOT_FOUND
#define ERROR_EVT_PUBLISHER_METADATA_NOT_FOUND 15028
#endif
#ifndef ERROR_EVT_CHANNEL_NOT_FOUND
#define ERROR_EVT_CHANNEL_NOT_FOUND 15007
#endif
#ifndef ERROR_EVT_QUERY_RESULT_STALE
#define ERROR_EVT_QUERY_RESULT_STALE 15031
#endif

static void *fail_metadata(void)
{
    SetLastError(ERROR_EVT_PUBLISHER_METADATA_NOT_FOUND);
    return NULL;
}

static void *fail_channel(void)
{
    SetLastError(ERROR_EVT_CHANNEL_NOT_FOUND);
    return NULL;
}

static void *fail_query(void)
{
    SetLastError(ERROR_EVT_QUERY_RESULT_STALE);
    return NULL;
}

BOOL WINAPI EvtClose(EHANDLE object)
{
    (void)object;
    SetLastError(ERROR_SUCCESS);
    return TRUE;
}

BOOL WINAPI EvtCancel(EHANDLE object)
{
    (void)object;
    SetLastError(ERROR_SUCCESS);
    return TRUE;
}

EHANDLE WINAPI EvtOpenPublisherMetadata(EVT_HANDLE session, LPCWSTR publisher_id,
                                        LPCWSTR log_file_path, LCID locale,
                                        DWORD flags)
{
    (void)session; (void)publisher_id; (void)log_file_path;
    (void)locale; (void)flags;
    return fail_metadata();
}

EHANDLE WINAPI EvtOpenPublisherEnum(EVT_HANDLE session, DWORD flags)
{
    (void)session; (void)flags;
    return fail_metadata();
}

EHANDLE WINAPI EvtOpenChannelEnum(EVT_HANDLE session, DWORD flags)
{
    (void)session; (void)flags;
    return fail_channel();
}

EHANDLE WINAPI EvtOpenLog(EVT_HANDLE session, LPCWSTR path, DWORD flags)
{
    (void)session; (void)path; (void)flags;
    return fail_channel();
}

EHANDLE WINAPI EvtQuery(EVT_HANDLE session, LPCWSTR path, LPCWSTR query,
                        DWORD flags)
{
    (void)session; (void)path; (void)query; (void)flags;
    return fail_channel();
}

EHANDLE WINAPI EvtSubscribe(EVT_HANDLE session, HANDLE signal_event,
                            LPCWSTR channel_path, LPCWSTR query,
                            EVT_HANDLE bookmark, PVOID context,
                            EVT_SUBSCRIBE_CALLBACK callback, DWORD flags)
{
    (void)session; (void)signal_event; (void)channel_path; (void)query;
    (void)bookmark; (void)context; (void)callback; (void)flags;
    return fail_channel();
}

BOOL WINAPI EvtNext(EVT_HANDLE result_set, DWORD size, EVT_HANDLE *events,
                    DWORD timeout, DWORD flags, PDWORD returned)
{
    (void)result_set; (void)size; (void)events; (void)timeout;
    (void)flags; (void)returned;
    return fail_query() != NULL;
}

BOOL WINAPI EvtRender(EVT_HANDLE context, EVT_HANDLE fragment, DWORD flags,
                      DWORD buffer_size, PVOID buffer, PDWORD buffer_used,
                      PDWORD property_count)
{
    (void)context; (void)fragment; (void)flags; (void)buffer_size;
    (void)buffer; (void)buffer_used; (void)property_count;
    SetLastError(ERROR_EVT_QUERY_RESULT_STALE);
    return FALSE;
}

BOOL WINAPI EvtGetExtendedStatus(DWORD buffer_size, LPWSTR buffer,
                                 PDWORD buffer_used)
{
    (void)buffer_size; (void)buffer; (void)buffer_used;
    SetLastError(ERROR_SUCCESS);
    return FALSE;
}

EVT_HANDLE WINAPI EvtCreateBookmark(LPCWSTR bookmark_xml)
{
    (void)bookmark_xml;
    return (EVT_HANDLE)fail_query();
}
