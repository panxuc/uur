/*
 * winlogon.exe — session presence marker.
 *
 * GameViewerService enumerates processes for a winlogon session token
 * source before it initialises; under plain Wine none exists and the
 * service never leaves its initial state (the launcher waits at the
 * splash forever).  This process only needs to exist with that name.
 */

#include <windows.h>

int WINAPI WinMain(HINSTANCE instance, HINSTANCE previous, LPSTR command_line,
                   int show_command)
{
    (void)instance;
    (void)previous;
    (void)command_line;
    (void)show_command;
    Sleep(INFINITE);
    return 0;
}
