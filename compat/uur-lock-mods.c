/* Process-local workaround for Wine/UU lock-key reconciliation. */
#define _GNU_SOURCE
#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <X11/XKBlib.h>
#include <X11/keysym.h>
#include <dlfcn.h>
#include <errno.h>
#include <limits.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

typedef Bool (*predicate_fn)(Display *, XEvent *, XPointer);
static pthread_once_t init_once = PTHREAD_ONCE_INIT;
static unsigned int requested_mask;
static int enabled;
static _Atomic unsigned long generation = 1;
static _Thread_local Display *cached_display;
static _Thread_local unsigned long cached_generation;
static _Thread_local unsigned int cached_mask;

#define EVENT_FUNCTIONS(X) \
    X(int, XNextEvent, (Display *d, XEvent *e), (d, e), rc == 0) \
    X(int, XPeekEvent, (Display *d, XEvent *e), (d, e), rc == 0) \
    X(int, XWindowEvent, (Display *d, Window w, long m, XEvent *e), (d, w, m, e), rc == 0) \
    X(Bool, XCheckWindowEvent, (Display *d, Window w, long m, XEvent *e), (d, w, m, e), rc != 0) \
    X(int, XMaskEvent, (Display *d, long m, XEvent *e), (d, m, e), rc == 0) \
    X(Bool, XCheckMaskEvent, (Display *d, long m, XEvent *e), (d, m, e), rc != 0) \
    X(Bool, XCheckTypedEvent, (Display *d, int t, XEvent *e), (d, t, e), rc != 0) \
    X(Bool, XCheckTypedWindowEvent, (Display *d, Window w, int t, XEvent *e), (d, w, t, e), rc != 0) \
    X(int, XIfEvent, (Display *d, XEvent *e, predicate_fn p, XPointer a), (d, e, p, a), rc == 0) \
    X(int, XPeekIfEvent, (Display *d, XEvent *e, predicate_fn p, XPointer a), (d, e, p, a), rc == 0) \
    X(Bool, XCheckIfEvent, (Display *d, XEvent *e, predicate_fn p, XPointer a), (d, e, p, a), rc != 0)

#define DECLARE(result, name, args, call, success) static result (*real_##name) args;
EVENT_FUNCTIONS(DECLARE)
#undef DECLARE
static int (*real_XRefreshKeyboardMapping)(XMappingEvent *);
static Status (*real_XkbRefreshKeyboardMapping)(XkbMapNotifyEvent *);
static int (*real_XCloseDisplay)(Display *);

static void *symbol(const char *name)
{
    void *address = dlsym(RTLD_NEXT, name);
    if (address == NULL) {
        fprintf(stderr, "uur keyboard compatibility: cannot resolve %s\n", name);
        abort();
    }
    return address;
}

static int wine_process(void)
{
    char executable[PATH_MAX];
    ssize_t size = readlink("/proc/self/exe", executable, sizeof(executable) - 1);
    if (size <= 0 || (size_t)size >= sizeof(executable) - 1)
        return 0;
    executable[size] = '\0';
    char *deleted = strstr(executable, " (deleted)");
    if (deleted != NULL)
        *deleted = '\0';
    char *name = strrchr(executable, '/');
    name = name == NULL ? executable : name + 1;
    if (*name == '.')
        name++;
    char *wrapped = strstr(name, "-wrapped");
    if (wrapped != NULL && wrapped[8] == '\0')
        *wrapped = '\0';
    return !strcmp(name, "wine") || !strcmp(name, "wine64") ||
           !strcmp(name, "wine-preloader") || !strcmp(name, "wine64-preloader") ||
           !strcmp(name, "wineserver");
}

static void initialize(void)
{
#define RESOLVE(result, name, args, call, success) real_##name = symbol(#name);
    EVENT_FUNCTIONS(RESOLVE)
#undef RESOLVE
    real_XRefreshKeyboardMapping = symbol("XRefreshKeyboardMapping");
    real_XkbRefreshKeyboardMapping = symbol("XkbRefreshKeyboardMapping");
    real_XCloseDisplay = symbol("XCloseDisplay");
    if (getenv("WINEPREFIX") == NULL || !wine_process())
        return;
    const char *mask = getenv("UUR_STRIP_LOCK_MODS");
    requested_mask = 0xff;
    if (mask != NULL && strcmp(mask, "auto")) {
        char *end;
        errno = 0;
        unsigned long value = strtoul(mask, &end,
            mask[0] == '0' && (mask[1] == 'x' || mask[1] == 'X') ? 16 : 10);
        if (errno || end == mask || *end != '\0' || value > 0xff) {
            fprintf(stderr, "uur keyboard compatibility: invalid modifier mask\n");
            return;
        }
        requested_mask = (unsigned int)value;
    }
    enabled = requested_mask != 0;
}

/* A modifier shared with a shortcut/layout key is left alone. CapsLock and
 * ShiftLock are never removed: their state affects character translation. */
static unsigned int locking_mask(Display *display)
{
    unsigned long current = atomic_load_explicit(&generation, memory_order_relaxed);
    if (cached_display == display && cached_generation == current)
        return cached_mask;
    unsigned int mask = 0;
    XModifierKeymap *mapping = XGetModifierMapping(display);
    if (mapping != NULL) {
        for (int modifier = 3; modifier < 8; ++modifier) {
            int locking = 0, other = 0;
            for (int key = 0; key < mapping->max_keypermod; ++key) {
                KeyCode code = mapping->modifiermap[modifier * mapping->max_keypermod + key];
                if (code == 0)
                    continue;
                int lock_key = 0;
                for (int level = 0; level < 4; ++level) {
                    KeySym sym = XkbKeycodeToKeysym(display, code, 0, level);
                    if (sym == XK_Num_Lock || sym == XK_Scroll_Lock)
                        lock_key = 1;
                    else if (sym != NoSymbol)
                        other = 1;
                }
                if (lock_key)
                    locking = 1;
                else
                    other = 1;
            }
            if (locking && !other)
                mask |= 1u << modifier;
        }
        XFreeModifiermap(mapping);
    }
    cached_display = display;
    cached_generation = current;
    cached_mask = mask & requested_mask;
    return cached_mask;
}

static void scrub(Display *display, XEvent *event)
{
    if (!enabled || event == NULL)
        return;
    if (event->type == MappingNotify) {
        atomic_fetch_add_explicit(&generation, 1, memory_order_relaxed);
        return;
    }
    if (event->type != KeyPress && event->type != KeyRelease)
        return;
    /* NumLock changes keypad virtual keys in EVENT_event_to_vkey(). Leave
     * those events intact rather than turning numeric input into navigation. */
    KeySym key = XkbKeycodeToKeysym(display, event->xkey.keycode, 0, 0);
    if (IsKeypadKey(key))
        return;
    event->xkey.state &= ~locking_mask(display);
}

#define IMPLEMENT(result, name, args, call, success) \
    result name args { \
        pthread_once(&init_once, initialize); \
        result rc = real_##name call; \
        if (success) scrub(d, e); \
        return rc; \
    }
EVENT_FUNCTIONS(IMPLEMENT)
#undef IMPLEMENT

int XRefreshKeyboardMapping(XMappingEvent *event)
{
    pthread_once(&init_once, initialize);
    int result = real_XRefreshKeyboardMapping(event);
    atomic_fetch_add_explicit(&generation, 1, memory_order_relaxed);
    return result;
}

Status XkbRefreshKeyboardMapping(XkbMapNotifyEvent *event)
{
    pthread_once(&init_once, initialize);
    Status result = real_XkbRefreshKeyboardMapping(event);
    atomic_fetch_add_explicit(&generation, 1, memory_order_relaxed);
    return result;
}

int XCloseDisplay(Display *display)
{
    pthread_once(&init_once, initialize);
    atomic_fetch_add_explicit(&generation, 1, memory_order_relaxed);
    return real_XCloseDisplay(display);
}
