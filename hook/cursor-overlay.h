#ifndef UUR_CURSOR_OVERLAY_H
#define UUR_CURSOR_OVERLAY_H

#include <limits.h>
#include <stdint.h>

typedef struct uur_cursor_rect {
    int x;
    int y;
    int width;
    int height;
} uur_cursor_rect;

typedef struct uur_cursor_shape {
    /* The hotspot is in the same screen coordinates as the GDI source DC. */
    int x;
    int y;
    int width;
    int height;
    int hotspot_x;
    int hotspot_y;
} uur_cursor_shape;

static inline int uur_cursor_bitmap_height(int mask_height, int has_color)
{
    return has_color ? mask_height : mask_height / 2;
}

static inline int uur_map_cursor_overlay(const uur_cursor_rect *source,
                                         const uur_cursor_rect *destination,
                                         const uur_cursor_shape *cursor,
                                         uur_cursor_rect *overlay)
{
    int64_t left;
    int64_t top;
    int64_t scaled_hotspot_x;
    int64_t scaled_hotspot_y;
    int64_t scaled_width;
    int64_t scaled_height;
    int64_t mapped_x;
    int64_t mapped_y;

    if (source->width <= 0 || source->height <= 0 ||
        destination->width <= 0 || destination->height <= 0 ||
        cursor->width <= 0 || cursor->height <= 0)
        return 0;

    left = (int64_t)cursor->x - cursor->hotspot_x;
    top = (int64_t)cursor->y - cursor->hotspot_y;
    /* Keep partially visible cursors at the edges of the captured region. */
    if (left >= (int64_t)source->x + source->width ||
        top >= (int64_t)source->y + source->height ||
        left + cursor->width <= source->x ||
        top + cursor->height <= source->y)
        return 0;

    scaled_hotspot_x = ((int64_t)cursor->hotspot_x * destination->width +
                        source->width / 2) / source->width;
    scaled_hotspot_y = ((int64_t)cursor->hotspot_y * destination->height +
                        source->height / 2) / source->height;
    scaled_width = ((int64_t)cursor->width * destination->width +
                    source->width / 2) / source->width;
    scaled_height = ((int64_t)cursor->height * destination->height +
                     source->height / 2) / source->height;

    mapped_x = (int64_t)destination->x +
        (((int64_t)cursor->x - source->x) * destination->width) /
        source->width - scaled_hotspot_x;
    mapped_y = (int64_t)destination->y +
        (((int64_t)cursor->y - source->y) * destination->height) /
        source->height - scaled_hotspot_y;
    if (mapped_x < INT_MIN || mapped_x > INT_MAX || mapped_y < INT_MIN ||
        mapped_y > INT_MAX || scaled_width > INT_MAX ||
        scaled_height > INT_MAX)
        return 0;
    overlay->x = (int)mapped_x;
    overlay->y = (int)mapped_y;
    overlay->width = scaled_width > 0 ? (int)scaled_width : 1;
    overlay->height = scaled_height > 0 ? (int)scaled_height : 1;
    return 1;
}

#endif
