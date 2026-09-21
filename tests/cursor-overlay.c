#include <assert.h>

#include "../hook/cursor-overlay.h"

int main(void)
{
    uur_cursor_rect source = {0, 0, 1920, 1080};
    uur_cursor_rect destination = {0, 0, 1920, 1080};
    uur_cursor_rect overlay;
    uur_cursor_shape cursor = {100, 200, 64, 48, 16, 8};

    /* A negative virtual-screen origin must not shift a primary-screen blit. */
    assert(uur_map_cursor_overlay(&source, &destination, &cursor, &overlay));
    assert(overlay.x == 84 && overlay.y == 192);
    assert(overlay.width == 64 && overlay.height == 48);

    source.x = -1920;
    cursor.x = -1820;
    assert(uur_map_cursor_overlay(&source, &destination, &cursor, &overlay));
    assert(overlay.x == 84);

    /* StretchBlt scales the cursor bitmap and hotspot together. */
    destination.width = 960;
    destination.height = 540;
    assert(uur_map_cursor_overlay(&source, &destination, &cursor, &overlay));
    assert(overlay.x == 42 && overlay.y == 96);
    assert(overlay.width == 32 && overlay.height == 24);

    /* A cursor overlapping the capture edge remains partially visible. */
    cursor.x = -1925;
    assert(uur_map_cursor_overlay(&source, &destination, &cursor, &overlay));
    cursor.x = -2000;
    assert(!uur_map_cursor_overlay(&source, &destination, &cursor, &overlay));

    assert(uur_cursor_bitmap_height(48, 1) == 48);
    assert(uur_cursor_bitmap_height(96, 0) == 48);
    return 0;
}
