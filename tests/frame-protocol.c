#include <assert.h>
#include <stddef.h>
#include <stdint.h>

#include "../hook/frame-protocol.h"

int main(void)
{
    const size_t slot_bytes = 8u + 1920u * 1080u * 4u;
    for (uint64_t sequence = 1; sequence < 1000; ++sequence) {
        size_t writer = uurf_slot_offset(sequence, slot_bytes);
        size_t reader = UURF_HEADER_BYTES +
            (size_t)(sequence % UURF_SLOT_COUNT) * slot_bytes;
        assert(writer == reader);
        assert(writer + slot_bytes <=
               UURF_HEADER_BYTES + UURF_SLOT_COUNT * slot_bytes);
    }
    return 0;
}
