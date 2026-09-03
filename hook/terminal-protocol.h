#ifndef UUR_TERMINAL_PROTOCOL_H
#define UUR_TERMINAL_PROTOCOL_H

#include <stdint.h>

#define UUR_TERMINAL_MAGIC 0x55555242u
#define UUR_TERMINAL_VERSION 1u
#define UUR_TERMINAL_TOKEN_BYTES 64u
#define UUR_TERMINAL_MAX_FRAME 65536u
#define UUR_TERMINAL_ACCEPTED 0x06u
#define UUR_TERMINAL_CONFIG "uu-terminal-bridge.runtime"

enum uur_terminal_frame_type {
    UUR_TERMINAL_DATA = 1,
    UUR_TERMINAL_RESIZE = 2,
    UUR_TERMINAL_EOF = 3,
};

#pragma pack(push, 1)
struct uur_terminal_hello {
    uint32_t magic;
    uint16_t version;
    uint16_t token_length;
    uint16_t columns;
    uint16_t rows;
};

struct uur_terminal_frame {
    uint8_t type;
    uint8_t reserved[3];
    uint32_t length;
};
#pragma pack(pop)

#endif
