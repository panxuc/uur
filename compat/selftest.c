#define _DEFAULT_SOURCE
/*
 * Self test for libuur-lock-mods.so.
 *
 * Delivers a synthetic KeyPress carrying Mod2|Mod4 through XCheckIfEvent -
 * the call winex11.drv actually uses to pump events - and reports the state
 * the application observes.  Needs an X display but no focus, no keyboard and
 * no desktop interaction.
 *
 * Exit codes: 0 = bits stripped, 1 = bits survived, 77 = environment unusable.
 */
#include <X11/Xlib.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define STRIPPED_BITS (0x10 | 0x40) /* Mod2 (NumLock) | Mod4 (Super) */

static Bool keep(Display *display, XEvent *event, XPointer arg)
{
    (void)display;
    (void)arg;
    return event->type == KeyPress;
}

int main(void)
{
    Display *display = XOpenDisplay(NULL);
    Window root, window;
    XEvent event;
    int received = 0;
    int state = 0;

    if (display == NULL) {
        fprintf(stderr, "no display\n");
        return 77;
    }

    root = DefaultRootWindow(display);
    window = XCreateSimpleWindow(display, root, 10, 10, 100, 100, 0, 0, 0xffffff);
    XSelectInput(display, window, KeyPressMask);
    XMapWindow(display, window);
    XSync(display, False);
    usleep(200000);

    memset(&event, 0, sizeof(event));
    event.xkey.type = KeyPress;
    event.xkey.display = display;
    event.xkey.window = window;
    event.xkey.root = root;
    event.xkey.subwindow = None;
    event.xkey.time = CurrentTime;
    event.xkey.x = 50;
    event.xkey.y = 50;
    event.xkey.state = STRIPPED_BITS;
    event.xkey.keycode = 38;
    event.xkey.same_screen = True;
    XSendEvent(display, window, True, KeyPressMask, &event);
    XSync(display, False);
    usleep(200000);

    while (XCheckIfEvent(display, &event, keep, NULL)) {
        received++;
        state = event.xkey.state;
    }

    printf("received=%d state=0x%02x\n", received, state);
    if (received == 0) {
        fprintf(stderr, "no event delivered; environment unusable\n");
        return 77;
    }
    if ((state & STRIPPED_BITS) == 0) {
        printf("ok: lock modifier bits stripped\n");
        return 0;
    }
    printf("failed: lock modifier bits survived (0x%02x)\n", state & STRIPPED_BITS);
    return 1;
}
