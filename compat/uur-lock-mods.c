/*
 * uur-lock-mods — stop Wine's X11 driver from forging lock-key events.
 *
 * Background
 * ----------
 * Wine keeps a Windows-side asynchronous keyboard state and reconciles it
 * with the modifier bits carried by every X11 key event
 * (dlls/winex11.drv/keyboard.c::update_lock_state()).  When the two disagree
 * it injects a synthetic lock-key down/up pair through SendInput to "catch
 * up", and if that pair does not change the async state it flips its own
 * bookkeeping and tries again on the next keystroke:
 *
 *     if (NtUserGetAsyncKeyboardState( keystate ) && (keystate[vkey] & 0x01) == prev_state)
 *     {
 *         WARN("keystate %x not changed (%#.2x), probably blocked by hooks\n", ...);
 *         keystate[vkey] ^= 0x01;
 *         set_async_key_state( keystate );
 *     }
 *
 * The UU Remote client installs a low-level keyboard hook that swallows lock
 * keys, so that compensation pair never lands and the mismatch persists: one
 * forged VK_NUMLOCK down/up per keystroke, forever.  The peer then receives
 * "NumLock + letter" instead of a plain letter.  On Windows peers that is
 * mostly harmless; on macOS, where the same modifier bit is interpreted as
 * Command, it turns every keystroke into a system shortcut.
 *
 * What this does
 * --------------
 * Loaded with LD_PRELOAD into the managed Wine processes, this shim strips
 * the lock modifier bits from the `state` field of delivered X key events
 * before the application sees them.  update_lock_state() then always agrees
 * with X, the compensation branch is never entered, and no synthetic key is
 * ever generated.  Because the bits are removed at the delivery boundary,
 * nothing outside these processes changes: the X server, the compositor and
 * the user's NumLock setting are untouched.
 *
 * AltGrMask in X11DRV_ToUnicodeEx() is derived from the same event->state, so
 * it stops being polluted by Mod2 as a side effect.
 *
 * Scope guard: the library is inherited by every child of the process that
 * loads it, so it stays inert unless the process looks like a managed Wine
 * process, or UUR_LOCKSTRIP_FORCE is set (used by the self test).
 *
 * Configuration
 * -------------
 *   UUR_STRIP_LOCK_MODS=<hex>  bits to strip; default 0x50 = Mod2 (NumLock)
 *                              | Mod4 (Super/Meta).  0 disables the shim.
 */
#define _GNU_SOURCE
#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <dlfcn.h>
#include <stdlib.h>
#include <string.h>

typedef int (*next_event_fn)(Display *, XEvent *);
typedef int (*window_event_fn)(Display *, Window, long, XEvent *);
typedef int (*check_window_event_fn)(Display *, Window, long, XEvent *);
typedef int (*mask_event_fn)(Display *, long, XEvent *);
typedef int (*check_mask_event_fn)(Display *, long, XEvent *);
typedef int (*check_typed_event_fn)(Display *, int, XEvent *);
typedef int (*check_typed_window_event_fn)(Display *, Window, int, XEvent *);
typedef int (*if_event_fn)(Display *, XEvent *, int (*)(Display *, XEvent *, XPointer), XPointer);
typedef int (*check_if_event_fn)(Display *, XEvent *, int (*)(Display *, XEvent *, XPointer), XPointer);

static unsigned long lock_mask;
static int initialised;

static int wanted(void)
{
    if (getenv("UUR_LOCKSTRIP_FORCE") != NULL) return 1;
    return getenv("WINEPREFIX") != NULL || getenv("WINEDLLOVERRIDES") != NULL;
}

static void init_mask(void)
{
    const char *env;

    if (initialised) return;
    initialised = 1;
    if (!wanted()) {
        lock_mask = 0;
        return;
    }
    env = getenv("UUR_STRIP_LOCK_MODS");
    lock_mask = env != NULL ? strtoul(env, NULL, 0) : (Mod2Mask | Mod4Mask);
}

static void scrub(XEvent *event)
{
    if (event == NULL) return;
    init_mask();
    if (lock_mask == 0) return;
    if (event->type == KeyPress || event->type == KeyRelease)
        event->xkey.state &= ~lock_mask;
}

static void *next_symbol(const char *name)
{
    return dlsym(RTLD_NEXT, name);
}

int XNextEvent(Display *display, XEvent *event)
{
    static next_event_fn fn;
    int rc;
    if (fn == NULL) fn = (next_event_fn)next_symbol("XNextEvent");
    rc = fn(display, event);
    if (rc == 0) scrub(event);
    return rc;
}

int XPeekEvent(Display *display, XEvent *event)
{
    static next_event_fn fn;
    int rc;
    if (fn == NULL) fn = (next_event_fn)next_symbol("XPeekEvent");
    rc = fn(display, event);
    if (rc == 0) scrub(event);
    return rc;
}

int XWindowEvent(Display *display, Window window, long mask, XEvent *event)
{
    static window_event_fn fn;
    int rc;
    if (fn == NULL) fn = (window_event_fn)next_symbol("XWindowEvent");
    rc = fn(display, window, mask, event);
    if (rc == 0) scrub(event);
    return rc;
}

int XCheckWindowEvent(Display *display, Window window, long mask, XEvent *event)
{
    static check_window_event_fn fn;
    int rc;
    if (fn == NULL) fn = (check_window_event_fn)next_symbol("XCheckWindowEvent");
    rc = fn(display, window, mask, event);
    if (rc != 0) scrub(event);
    return rc;
}

int XMaskEvent(Display *display, long mask, XEvent *event)
{
    static mask_event_fn fn;
    int rc;
    if (fn == NULL) fn = (mask_event_fn)next_symbol("XMaskEvent");
    rc = fn(display, mask, event);
    if (rc == 0) scrub(event);
    return rc;
}

int XCheckMaskEvent(Display *display, long mask, XEvent *event)
{
    static check_mask_event_fn fn;
    int rc;
    if (fn == NULL) fn = (check_mask_event_fn)next_symbol("XCheckMaskEvent");
    rc = fn(display, mask, event);
    if (rc != 0) scrub(event);
    return rc;
}

int XCheckTypedEvent(Display *display, int type, XEvent *event)
{
    static check_typed_event_fn fn;
    int rc;
    if (fn == NULL) fn = (check_typed_event_fn)next_symbol("XCheckTypedEvent");
    rc = fn(display, type, event);
    if (rc != 0) scrub(event);
    return rc;
}

int XCheckTypedWindowEvent(Display *display, Window window, int type, XEvent *event)
{
    static check_typed_window_event_fn fn;
    int rc;
    if (fn == NULL) fn = (check_typed_window_event_fn)next_symbol("XCheckTypedWindowEvent");
    rc = fn(display, window, type, event);
    if (rc != 0) scrub(event);
    return rc;
}

/* winex11.drv pumps events through XCheckIfEvent (event.c::X11DRV_ProcessEvents);
 * this pair is what actually matters in practice. */
int XCheckIfEvent(Display *display, XEvent *event,
                  Bool (*predicate)(Display *, XEvent *, XPointer), XPointer arg)
{
    static check_if_event_fn fn;
    int rc;
    if (fn == NULL) fn = (check_if_event_fn)next_symbol("XCheckIfEvent");
    rc = fn(display, event, predicate, arg);
    if (rc != 0) scrub(event);
    return rc;
}

int XIfEvent(Display *display, XEvent *event,
             Bool (*predicate)(Display *, XEvent *, XPointer), XPointer arg)
{
    static if_event_fn fn;
    int rc;
    if (fn == NULL) fn = (if_event_fn)next_symbol("XIfEvent");
    rc = fn(display, event, predicate, arg);
    scrub(event);
    return rc;
}
