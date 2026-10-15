#include "PrivateMusicMedia.h"

#include <CommonCrypto/CommonCrypto.h>
#include <math.h>
#include <stdint.h>
#include <string.h>

#pragma mark - MPEG-TS helpers

static bool pm_mpegts_looks_adts(const uint8_t *data, int32_t length) {
    if (data == NULL || length < 2) {
        return false;
    }
    return data[0] == 0xFF && (data[1] & 0xF6) == 0xF0;
}

static bool pm_mpegts_looks_mp3(const uint8_t *data, int32_t length) {
    if (data == NULL || length < 3) {
        return false;
    }
    if (data[0] == 0x49 && data[1] == 0x44 && data[2] == 0x33) {
        return true;
    }
    if (data[0] != 0xFF) {
        return false;
    }
    const int version_bits = (data[1] >> 3) & 0x03;
    const int layer_bits = (data[1] >> 1) & 0x03;
    if (version_bits == 1 || layer_bits == 0) {
        return false;
    }
    return (data[1] & 0xE0) == 0xE0;
}

static int32_t pm_mpegts_infer_kind(const uint8_t *data, int32_t length) {
    if (pm_mpegts_looks_adts(data, length)) {
        return PM_MPEGTS_KIND_ADTS;
    }
    if (pm_mpegts_looks_mp3(data, length)) {
        return PM_MPEGTS_KIND_MP3;
    }
    return PM_MPEGTS_KIND_NONE;
}

static int32_t pm_mpegts_kind_for_stream_type(uint8_t stream_type) {
    switch (stream_type) {
        case 0x0F:
            return PM_MPEGTS_KIND_ADTS;
        case 0x03:
        case 0x04:
            return PM_MPEGTS_KIND_MP3;
        default:
            return PM_MPEGTS_KIND_NONE;
    }
}

int32_t pm_mpegts_packet_size(const uint8_t *data, int32_t length) {
    if (data == NULL || length <= 0) {
        return 0;
    }
    const int32_t sizes[3] = {188, 192, 204};
    for (int size_index = 0; size_index < 3; size_index++) {
        const int32_t size = sizes[size_index];
        int32_t syncs = 0;
        int32_t offset = 0;
        while (offset + size <= length) {
            if (data[offset] != 0x47) {
                break;
            }
            syncs += 1;
            if (syncs >= 3) {
                return size;
            }
            offset += size;
        }
        if (syncs >= 2 && offset >= length) {
            return size;
        }
    }
    return 0;
}

bool pm_mpegts_parse_packet(
    const uint8_t *packet,
    int32_t packet_size,
    int32_t *pid,
    bool *unit_start,
    int32_t *payload_offset,
    int32_t *payload_length
) {
    if (packet == NULL || packet_size < 4 || packet[0] != 0x47) {
        return false;
    }
    const int32_t parsed_pid = ((packet[1] & 0x1F) << 8) | packet[2];
    const bool parsed_start = (packet[1] & 0x40) != 0;
    const int adaptation = (packet[3] >> 4) & 0x03;
    int32_t offset = 4;
    if (adaptation == 2 || adaptation == 3) {
        if (offset >= packet_size) {
            return false;
        }
        offset += 1 + (int32_t)packet[offset];
    }
    if (adaptation != 1 && adaptation != 3) {
        return false;
    }
    if (offset >= packet_size) {
        return false;
    }
    if (pid != NULL) {
        *pid = parsed_pid;
    }
    if (unit_start != NULL) {
        *unit_start = parsed_start;
    }
    if (payload_offset != NULL) {
        *payload_offset = offset;
    }
    if (payload_length != NULL) {
        *payload_length = packet_size - offset;
    }
    return true;
}

static const uint8_t *pm_mpegts_psi_section(
    const uint8_t *payload,
    int32_t length,
    int32_t *section_length
) {
    if (payload == NULL || length <= 0) {
        return NULL;
    }
    const int32_t pointer = payload[0];
    const int32_t start = 1 + pointer;
    if (start >= length) {
        return NULL;
    }
    if (section_length != NULL) {
        *section_length = length - start;
    }
    return payload + start;
}

int32_t pm_mpegts_parse_pat(
    const uint8_t *payload,
    int32_t length
) {
    int32_t section_length = 0;
    const uint8_t *section = pm_mpegts_psi_section(
        payload,
        length,
        &section_length
    );
    if (section == NULL || section_length < 8 || section[0] != 0x00) {
        return -1;
    }
    const int32_t declared = ((section[1] & 0x0F) << 8) | section[2];
    const int32_t section_end = 3 + declared;
    if (section_length < section_end || section_end < 12) {
        return -1;
    }
    int32_t offset = 8;
    const int32_t end = section_end - 4;
    while (offset + 4 <= end) {
        const int32_t program_number = (section[offset] << 8)
            | section[offset + 1];
        const int32_t pid = ((section[offset + 2] & 0x1F) << 8)
            | section[offset + 3];
        offset += 4;
        if (program_number != 0) {
            return pid;
        }
    }
    return -1;
}

bool pm_mpegts_parse_pmt(
    const uint8_t *payload,
    int32_t length,
    int32_t *audio_pid,
    uint8_t *stream_type
) {
    int32_t section_length = 0;
    const uint8_t *section = pm_mpegts_psi_section(
        payload,
        length,
        &section_length
    );
    if (section == NULL || section_length < 12 || section[0] != 0x02) {
        return false;
    }
    const int32_t declared = ((section[1] & 0x0F) << 8) | section[2];
    const int32_t section_end = 3 + declared;
    if (section_length < section_end || section_end < 16) {
        return false;
    }
    const int32_t program_info_length = ((section[10] & 0x0F) << 8)
        | section[11];
    int32_t offset = 12 + program_info_length;
    const int32_t end = section_end - 4;
    bool have_preferred = false;
    int32_t preferred_pid = 0;
    uint8_t preferred_type = 0;
    while (offset + 5 <= end) {
        const uint8_t type = section[offset];
        const int32_t elementary_pid = ((section[offset + 1] & 0x1F) << 8)
            | section[offset + 2];
        const int32_t es_info_length = ((section[offset + 3] & 0x0F) << 8)
            | section[offset + 4];
        offset += 5 + es_info_length;
        const int32_t kind = pm_mpegts_kind_for_stream_type(type);
        if (kind == PM_MPEGTS_KIND_NONE) {
            continue;
        }
        if (kind == PM_MPEGTS_KIND_ADTS) {
            if (audio_pid != NULL) {
                *audio_pid = elementary_pid;
            }
            if (stream_type != NULL) {
                *stream_type = type;
            }
            return true;
        }
        if (!have_preferred) {
            have_preferred = true;
            preferred_pid = elementary_pid;
            preferred_type = type;
        }
    }
    if (!have_preferred) {
        return false;
    }
    if (audio_pid != NULL) {
        *audio_pid = preferred_pid;
    }
    if (stream_type != NULL) {
        *stream_type = preferred_type;
    }
    return true;
}

bool pm_mpegts_pes_slice(
    const uint8_t *payload,
    int32_t payload_length,
    bool unit_start,
    int32_t *remaining,
    int32_t *out_offset,
    int32_t *out_length
) {
    if (payload == NULL || remaining == NULL || payload_length < 0) {
        return false;
    }
    int32_t start = 0;
    if (unit_start) {
        if (payload_length < 9
            || payload[0] != 0
            || payload[1] != 0
            || payload[2] != 1) {
            *remaining = PM_MPEGTS_PES_UNBOUNDED;
            return false;
        }
        const int32_t packet_length = (payload[4] << 8) | payload[5];
        const int32_t header_length = payload[8];
        const int32_t payload_start = 9 + header_length;
        if (payload_start > payload_length) {
            *remaining = PM_MPEGTS_PES_UNBOUNDED;
            return false;
        }
        *remaining = packet_length == 0
            ? PM_MPEGTS_PES_UNBOUNDED
            : packet_length - 3 - header_length;
        if (*remaining < 0) {
            *remaining = 0;
        }
        start = payload_start;
    } else if (*remaining == 0) {
        return false;
    }
    const int32_t available = payload_length - start;
    if (available <= 0) {
        return false;
    }
    int32_t count = available;
    if (*remaining != PM_MPEGTS_PES_UNBOUNDED && *remaining < count) {
        count = *remaining;
    }
    if (out_offset != NULL) {
        *out_offset = start;
    }
    if (out_length != NULL) {
        *out_length = count;
    }
    if (*remaining != PM_MPEGTS_PES_UNBOUNDED) {
        *remaining -= count;
        if (*remaining < 0) {
            *remaining = 0;
        }
    }
    return count > 0;
}

static int32_t pm_mpegts_find_pes(const uint8_t *data, int32_t length) {
    if (data == NULL || length < 4) {
        return -1;
    }
    for (int32_t index = 0; index + 3 < length; index++) {
        if (data[index] == 0x00
            && data[index + 1] == 0x00
            && data[index + 2] == 0x01) {
            return index;
        }
    }
    return -1;
}

static void pm_mpegts_consume(uint8_t *buffer, int32_t *length, int32_t count) {
    if (buffer == NULL || length == NULL || count <= 0) {
        return;
    }
    if (count >= *length) {
        *length = 0;
        return;
    }
    memmove(buffer, buffer + count, (size_t)(*length - count));
    *length -= count;
}

/// Returns consumed bytes, 0 when the caller should stop draining, -1 when
/// more payload is required.
static int32_t pm_mpegts_drain_pes(
    uint8_t *buffer,
    int32_t *buffer_length,
    uint8_t *elementary,
    int32_t elementary_capacity,
    int32_t *elementary_length,
    bool audio_only,
    bool allow_partial
) {
    if (buffer == NULL || buffer_length == NULL) {
        return 0;
    }
    const int32_t start = pm_mpegts_find_pes(buffer, *buffer_length);
    if (start < 0) {
        if (*buffer_length > 10000) {
            pm_mpegts_consume(buffer, buffer_length, *buffer_length - 3);
        }
        return 0;
    }
    if (start > 0) {
        pm_mpegts_consume(buffer, buffer_length, start);
    }
    if (*buffer_length < 9) {
        return allow_partial ? 0 : -1;
    }
    const uint8_t stream_id = buffer[3];
    if (audio_only && (stream_id < 0xC0 || stream_id > 0xDF)) {
        pm_mpegts_consume(buffer, buffer_length, 4);
        return 4;
    }
    const int32_t pes_packet_length = (buffer[4] << 8) | buffer[5];
    const int32_t header_data_length = buffer[8];
    const int32_t payload_start = 9 + header_data_length;
    if (*buffer_length < payload_start) {
        return allow_partial ? 0 : -1;
    }
    int32_t packet_end;
    if (pes_packet_length == 0) {
        if (!allow_partial) {
            return -1;
        }
        packet_end = *buffer_length;
    } else {
        packet_end = 6 + pes_packet_length;
        if (*buffer_length < packet_end && !allow_partial) {
            return -1;
        }
    }
    const int32_t end = packet_end < *buffer_length ? packet_end : *buffer_length;
    if (payload_start < end
        && elementary != NULL
        && elementary_length != NULL) {
        const int32_t copy = end - payload_start;
        if (*elementary_length >= 0
            && copy > 0
            && *elementary_length + copy <= elementary_capacity) {
            memcpy(
                elementary + *elementary_length,
                buffer + payload_start,
                (size_t)copy
            );
            *elementary_length += copy;
        }
    }
    pm_mpegts_consume(buffer, buffer_length, end);
    return end;
}

static void pm_mpegts_append_and_drain(
    uint8_t *pes,
    int32_t *pes_length,
    int32_t pes_capacity,
    const uint8_t *payload,
    int32_t payload_length,
    uint8_t *elementary,
    int32_t elementary_capacity,
    int32_t *elementary_length,
    bool audio_only
) {
    if (payload != NULL && payload_length > 0
        && *pes_length + payload_length <= pes_capacity) {
        memcpy(pes + *pes_length, payload, (size_t)payload_length);
        *pes_length += payload_length;
    }
    for (;;) {
        const int32_t consumed = pm_mpegts_drain_pes(
            pes,
            pes_length,
            elementary,
            elementary_capacity,
            elementary_length,
            audio_only,
            false
        );
        if (consumed <= 0) {
            break;
        }
    }
}

int32_t pm_mpegts_extract_audio(
    const uint8_t *data,
    int32_t length,
    uint8_t *out,
    int32_t out_capacity,
    uint8_t *scratch,
    int32_t scratch_capacity,
    int32_t *out_length
) {
    if (out_length != NULL) {
        *out_length = 0;
    }
    if (data == NULL || length <= 0 || out == NULL || scratch == NULL) {
        return PM_MPEGTS_KIND_NONE;
    }
    const int32_t packet_size = pm_mpegts_packet_size(data, length);
    if (packet_size <= 0) {
        return PM_MPEGTS_KIND_NONE;
    }

    int32_t pmt_pid = -1;
    int32_t offset = 0;
    while (offset + packet_size <= length) {
        int32_t pid = 0;
        bool unit_start = false;
        int32_t payload_offset = 0;
        int32_t payload_length = 0;
        if (pm_mpegts_parse_packet(
                data + offset,
                packet_size,
                &pid,
                &unit_start,
                &payload_offset,
                &payload_length
            )
            && pid == 0
            && pmt_pid < 0) {
            pmt_pid = pm_mpegts_parse_pat(
                data + offset + payload_offset,
                payload_length
            );
        }
        offset += packet_size;
    }

    int32_t audio_pid = -1;
    uint8_t stream_type = 0;
    if (pmt_pid >= 0) {
        offset = 0;
        while (offset + packet_size <= length) {
            int32_t pid = 0;
            bool unit_start = false;
            int32_t payload_offset = 0;
            int32_t payload_length = 0;
            if (pm_mpegts_parse_packet(
                    data + offset,
                    packet_size,
                    &pid,
                    &unit_start,
                    &payload_offset,
                    &payload_length
                )
                && pid == pmt_pid
                && pm_mpegts_parse_pmt(
                    data + offset + payload_offset,
                    payload_length,
                    &audio_pid,
                    &stream_type
                )) {
                break;
            }
            offset += packet_size;
        }
    }

    const bool audio_only = audio_pid < 0;
    int32_t pes_length = 0;
    int32_t elementary_length = 0;
    offset = 0;
    while (offset + packet_size <= length) {
        int32_t pid = 0;
        bool unit_start = false;
        int32_t payload_offset = 0;
        int32_t payload_length = 0;
        if (pm_mpegts_parse_packet(
                data + offset,
                packet_size,
                &pid,
                &unit_start,
                &payload_offset,
                &payload_length
            )
            && (audio_only || pid == audio_pid)) {
            pm_mpegts_append_and_drain(
                scratch,
                &pes_length,
                scratch_capacity,
                data + offset + payload_offset,
                payload_length,
                out,
                out_capacity,
                &elementary_length,
                audio_only
            );
        }
        offset += packet_size;
    }
    (void)pm_mpegts_drain_pes(
        scratch,
        &pes_length,
        out,
        out_capacity,
        &elementary_length,
        audio_only,
        true
    );

    if (elementary_length <= 32) {
        return PM_MPEGTS_KIND_NONE;
    }
    int32_t kind = pm_mpegts_kind_for_stream_type(stream_type);
    if (kind == PM_MPEGTS_KIND_NONE) {
        kind = pm_mpegts_infer_kind(out, elementary_length);
    }
    if (kind == PM_MPEGTS_KIND_NONE) {
        return PM_MPEGTS_KIND_NONE;
    }
    if (out_length != NULL) {
        *out_length = elementary_length;
    }
    return kind;
}

#pragma mark - ISO BMFF

uint32_t pm_iso_fourcc(const char *type) {
    if (type == NULL) {
        return 0;
    }
    uint32_t value = 0;
    for (int index = 0; index < 4; index++) {
        const unsigned char byte = (unsigned char)type[index];
        if (byte == 0) {
            break;
        }
        value = (value << 8) | byte;
    }
    return value;
}

static uint16_t pm_iso_load_u16(const uint8_t *data, int32_t offset) {
    return (uint16_t)(((uint16_t)data[offset] << 8) | (uint16_t)data[offset + 1]);
}

static uint32_t pm_iso_load_u32(const uint8_t *data, int32_t offset) {
    return ((uint32_t)data[offset] << 24)
        | ((uint32_t)data[offset + 1] << 16)
        | ((uint32_t)data[offset + 2] << 8)
        | (uint32_t)data[offset + 3];
}

static uint64_t pm_iso_load_u64(const uint8_t *data, int32_t offset) {
    return ((uint64_t)pm_iso_load_u32(data, offset) << 32)
        | (uint64_t)pm_iso_load_u32(data, offset + 4);
}

static int32_t pm_iso_load_i32(const uint8_t *data, int32_t offset) {
    return (int32_t)pm_iso_load_u32(data, offset);
}

static uint32_t pm_iso_load_u24(const uint8_t *data, int32_t offset) {
    return ((uint32_t)data[offset] << 16)
        | ((uint32_t)data[offset + 1] << 8)
        | (uint32_t)data[offset + 2];
}

static bool pm_iso_parse_box(
    const uint8_t *data,
    int32_t length,
    int32_t offset,
    int32_t limit,
    pm_iso_box *out,
    pm_iso_status *status
) {
    if (offset < 0 || offset + 8 > length || offset + 8 > limit) {
        if (status != NULL) {
            status->code = PM_ISO_TRUNCATED;
            status->expected = 8;
            status->available = length - (offset < 0 ? 0 : offset);
            if (status->available < 0) {
                status->available = 0;
            }
        }
        return false;
    }
    const uint32_t size32 = pm_iso_load_u32(data, offset);
    const uint32_t type = pm_iso_load_u32(data, offset + 4);
    int32_t header_size = 8;
    int64_t size = (int64_t)size32;
    if (size32 == 0) {
        if (offset + 8 > limit) {
            if (status != NULL) {
                status->code = PM_ISO_TRUNCATED;
                status->expected = 8;
                status->available = limit - offset;
                if (status->available < 0) {
                    status->available = 0;
                }
            }
            return false;
        }
        if (out != NULL) {
            out->type = type;
            out->start = offset;
            out->size = limit - offset;
            out->header_size = 8;
        }
        if (status != NULL) {
            status->code = PM_ISO_OK;
            status->expected = 0;
            status->available = 0;
        }
        return true;
    }
    if (size32 == 1) {
        if (offset + 16 > length) {
            if (status != NULL) {
                status->code = PM_ISO_TRUNCATED;
                status->expected = 16;
                status->available = length - offset;
                if (status->available < 0) {
                    status->available = 0;
                }
            }
            return false;
        }
        const uint64_t size64 = pm_iso_load_u64(data, offset + 8);
        if (size64 > (uint64_t)INT32_MAX || size64 < 16) {
            if (status != NULL) {
                status->code = PM_ISO_INVALID;
                status->expected = 0;
                status->available = 0;
            }
            return false;
        }
        header_size = 16;
        size = (int64_t)size64;
    } else if (size < 8) {
        if (status != NULL) {
            status->code = PM_ISO_INVALID;
            status->expected = 0;
            status->available = 0;
        }
        return false;
    }
    const int32_t remaining = limit - offset;
    if (remaining < 0 || size > remaining) {
        if (status != NULL) {
            status->code = PM_ISO_TRUNCATED;
            status->expected = (int32_t)size;
            status->available = remaining < 0 ? 0 : remaining;
        }
        return false;
    }
    if (out != NULL) {
        out->type = type;
        out->start = offset;
        out->size = (int32_t)size;
        out->header_size = header_size;
    }
    if (status != NULL) {
        status->code = PM_ISO_OK;
        status->expected = 0;
        status->available = 0;
    }
    return true;
}

int32_t pm_iso_walk(
    const uint8_t *data,
    int32_t length,
    int32_t start,
    int32_t end,
    pm_iso_box *out,
    int32_t out_capacity,
    pm_iso_status *status
) {
    pm_iso_status local = {PM_ISO_OK, 0, 0};
    pm_iso_status *st = status != NULL ? status : &local;
    st->code = PM_ISO_OK;
    st->expected = 0;
    st->available = 0;
    if (data == NULL || start < 0 || start > end || end > length) {
        st->code = PM_ISO_TRUNCATED;
        st->expected = end - start;
        if (st->expected < 0) {
            st->expected = 0;
        }
        st->available = length - start;
        if (st->available < 0) {
            st->available = 0;
        }
        return 0;
    }
    int32_t written = 0;
    int32_t offset = start;
    int32_t count = 0;
    while (offset + 8 <= end && count < PM_ISO_WALK_LIMIT) {
        count += 1;
        pm_iso_box box;
        if (!pm_iso_parse_box(data, length, offset, end, &box, st)) {
            return written;
        }
        if (out != NULL && written < out_capacity) {
            out[written] = box;
        }
        written += 1;
        if (box.size <= 0) {
            break;
        }
        if (box.size > end - offset) {
            break;
        }
        offset += box.size;
    }
    st->code = PM_ISO_OK;
    return written;
}

bool pm_iso_contains_types(
    const uint8_t *data,
    int32_t length,
    const uint32_t *types,
    int32_t type_count
) {
    if (type_count <= 0) {
        return true;
    }
    if (data == NULL || types == NULL || length < 8) {
        return false;
    }
    uint32_t remaining[16];
    int32_t remaining_count = type_count;
    if (remaining_count > 16) {
        remaining_count = 16;
    }
    memcpy(remaining, types, (size_t)remaining_count * sizeof(uint32_t));

    int32_t offset = 0;
    int32_t walked = 0;
    while (offset + 8 <= length
        && remaining_count > 0
        && walked < PM_ISO_WALK_LIMIT) {
        walked += 1;
        const uint32_t size32 = pm_iso_load_u32(data, offset);
        const uint32_t type = pm_iso_load_u32(data, offset + 4);
        for (int32_t type_index = 0; type_index < remaining_count; type_index++) {
            if (type != remaining[type_index]) {
                continue;
            }
            remaining[type_index] = remaining[remaining_count - 1];
            remaining_count -= 1;
            break;
        }
        if (remaining_count == 0) {
            return true;
        }

        int64_t box_size;
        if (size32 == 1) {
            if (offset + 16 > length) {
                break;
            }
            const uint64_t size64 = pm_iso_load_u64(data, offset + 8);
            box_size = size64 > (uint64_t)INT32_MAX
                ? (int64_t)(length - offset)
                : (int64_t)size64;
        } else if (size32 == 0) {
            box_size = (int64_t)(length - offset);
        } else {
            box_size = (int64_t)size32;
        }
        if (box_size < 8) {
            break;
        }
        if (box_size > (int64_t)(length - offset)) {
            break;
        }
        offset += (int32_t)box_size;
    }
    return remaining_count == 0;
}

#pragma mark - CMAF fragment extract

#define PM_ISO_TYPE(a, b, c, d) \
    (((uint32_t)(unsigned char)(a) << 24) \
        | ((uint32_t)(unsigned char)(b) << 16) \
        | ((uint32_t)(unsigned char)(c) << 8) \
        | (uint32_t)(unsigned char)(d))

#define PM_ISO_FTYP PM_ISO_TYPE('f', 't', 'y', 'p')
#define PM_ISO_MOOV PM_ISO_TYPE('m', 'o', 'o', 'v')
#define PM_ISO_TRAK PM_ISO_TYPE('t', 'r', 'a', 'k')
#define PM_ISO_TKHD PM_ISO_TYPE('t', 'k', 'h', 'd')
#define PM_ISO_MDIA PM_ISO_TYPE('m', 'd', 'i', 'a')
#define PM_ISO_MDHD PM_ISO_TYPE('m', 'd', 'h', 'd')
#define PM_ISO_HDLR PM_ISO_TYPE('h', 'd', 'l', 'r')
#define PM_ISO_MINF PM_ISO_TYPE('m', 'i', 'n', 'f')
#define PM_ISO_STBL PM_ISO_TYPE('s', 't', 'b', 'l')
#define PM_ISO_STSD PM_ISO_TYPE('s', 't', 's', 'd')
#define PM_ISO_ESDS PM_ISO_TYPE('e', 's', 'd', 's')
#define PM_ISO_MVEX PM_ISO_TYPE('m', 'v', 'e', 'x')
#define PM_ISO_TREX PM_ISO_TYPE('t', 'r', 'e', 'x')
#define PM_ISO_SOUN PM_ISO_TYPE('s', 'o', 'u', 'n')
#define PM_ISO_MOOF PM_ISO_TYPE('m', 'o', 'o', 'f')
#define PM_ISO_MDAT PM_ISO_TYPE('m', 'd', 'a', 't')
#define PM_ISO_TRAF PM_ISO_TYPE('t', 'r', 'a', 'f')
#define PM_ISO_TFHD PM_ISO_TYPE('t', 'f', 'h', 'd')
#define PM_ISO_TFDT PM_ISO_TYPE('t', 'f', 'd', 't')
#define PM_ISO_TRUN PM_ISO_TYPE('t', 'r', 'u', 'n')

#define PM_CMAF_BOX_CAP 64

static void pm_cmaf_fail(pm_cmaf_status *status, int32_t code, uint32_t track) {
    if (status == NULL) {
        return;
    }
    status->code = code;
    status->found_track_id = track;
}

static bool pm_cmaf_fits(
    int32_t offset,
    int32_t bytes,
    int32_t limit
) {
    return offset >= 0 && bytes >= 0 && offset <= limit - bytes;
}

static int32_t pm_cmaf_payload_start(const pm_iso_box *box) {
    return box->start + box->header_size;
}

static int32_t pm_cmaf_payload_end(const pm_iso_box *box) {
    return box->start + box->size;
}

static const pm_iso_box *pm_cmaf_find_box(
    const pm_iso_box *boxes,
    int32_t count,
    uint32_t type
) {
    for (int32_t index = 0; index < count; index++) {
        if (boxes[index].type == type) {
            return &boxes[index];
        }
    }
    return NULL;
}

static int32_t pm_cmaf_walk_children(
    const uint8_t *data,
    int32_t length,
    const pm_iso_box *parent,
    pm_iso_box *out,
    int32_t out_capacity,
    pm_cmaf_status *status
) {
    pm_iso_status walk = {PM_ISO_OK, 0, 0};
    const int32_t count = pm_iso_walk(
        data,
        length,
        pm_cmaf_payload_start(parent),
        pm_cmaf_payload_end(parent),
        out,
        out_capacity,
        &walk
    );
    if (walk.code != PM_ISO_OK && count == 0) {
        pm_cmaf_fail(
            status,
            walk.code == PM_ISO_TRUNCATED
                ? PM_CMAF_TRUNCATED
                : PM_CMAF_MISSING_MOOF,
            0
        );
        return -1;
    }
    return count;
}

typedef struct {
    uint32_t track_id;
    uint32_t default_duration;
    uint32_t default_size;
    uint32_t default_flags;
    bool has_duration;
    bool has_size;
    bool has_flags;
} pm_cmaf_tfhd;

static bool pm_cmaf_parse_tfhd(
    const uint8_t *data,
    int32_t length,
    const pm_iso_box *box,
    pm_cmaf_tfhd *out,
    pm_cmaf_status *status
) {
    const int32_t body = pm_cmaf_payload_start(box);
    const int32_t end = pm_cmaf_payload_end(box);
    if (!pm_cmaf_fits(body, 8, end) || !pm_cmaf_fits(body, 8, length)) {
        pm_cmaf_fail(status, PM_CMAF_TRUNCATED, 0);
        return false;
    }
    const uint32_t flags = pm_iso_load_u24(data, body + 1);
    int32_t offset = body + 8;
    pm_cmaf_tfhd header;
    header.track_id = pm_iso_load_u32(data, body + 4);
    header.default_duration = 0;
    header.default_size = 0;
    header.default_flags = 0;
    header.has_duration = false;
    header.has_size = false;
    header.has_flags = false;
    if (flags & 0x000001) {
        if (!pm_cmaf_fits(offset, 8, end)) {
            pm_cmaf_fail(status, PM_CMAF_TRUNCATED, header.track_id);
            return false;
        }
        offset += 8;
    }
    if (flags & 0x000002) {
        if (!pm_cmaf_fits(offset, 4, end)) {
            pm_cmaf_fail(status, PM_CMAF_TRUNCATED, header.track_id);
            return false;
        }
        offset += 4;
    }
    if (flags & 0x000008) {
        if (!pm_cmaf_fits(offset, 4, end)) {
            pm_cmaf_fail(status, PM_CMAF_TRUNCATED, header.track_id);
            return false;
        }
        header.default_duration = pm_iso_load_u32(data, offset);
        header.has_duration = true;
        offset += 4;
    }
    if (flags & 0x000010) {
        if (!pm_cmaf_fits(offset, 4, end)) {
            pm_cmaf_fail(status, PM_CMAF_TRUNCATED, header.track_id);
            return false;
        }
        header.default_size = pm_iso_load_u32(data, offset);
        header.has_size = true;
        offset += 4;
    }
    if (flags & 0x000020) {
        if (!pm_cmaf_fits(offset, 4, end)) {
            pm_cmaf_fail(status, PM_CMAF_TRUNCATED, header.track_id);
            return false;
        }
        header.default_flags = pm_iso_load_u32(data, offset);
        header.has_flags = true;
    }
    *out = header;
    return true;
}

static bool pm_cmaf_parse_tfdt(
    const uint8_t *data,
    int32_t length,
    const pm_iso_box *box,
    int64_t *out,
    pm_cmaf_status *status
) {
    const int32_t body = pm_cmaf_payload_start(box);
    const int32_t end = pm_cmaf_payload_end(box);
    if (!pm_cmaf_fits(body, 5, end) || !pm_cmaf_fits(body, 5, length)) {
        pm_cmaf_fail(status, PM_CMAF_TRUNCATED, 0);
        return false;
    }
    const uint8_t version = data[body];
    if (version == 1) {
        if (!pm_cmaf_fits(body + 4, 8, end)) {
            pm_cmaf_fail(status, PM_CMAF_TRUNCATED, 0);
            return false;
        }
        *out = (int64_t)pm_iso_load_u64(data, body + 4);
        return true;
    }
    if (!pm_cmaf_fits(body + 4, 4, end)) {
        pm_cmaf_fail(status, PM_CMAF_TRUNCATED, 0);
        return false;
    }
    *out = (int64_t)pm_iso_load_u32(data, body + 4);
    return true;
}

static bool pm_cmaf_store_sample(
    pm_cmaf_sample *out,
    int32_t out_capacity,
    int32_t *written,
    pm_cmaf_sample sample,
    pm_cmaf_status *status
) {
    if (*written >= PM_CMAF_SAMPLE_LIMIT) {
        pm_cmaf_fail(status, PM_CMAF_OVERFLOW, 0);
        return false;
    }
    if (out != NULL) {
        if (*written >= out_capacity) {
            pm_cmaf_fail(status, PM_CMAF_OVERFLOW, 0);
            return false;
        }
        out[*written] = sample;
    }
    *written += 1;
    return true;
}

static bool pm_cmaf_extract_trun(
    const uint8_t *data,
    int32_t length,
    const pm_iso_box *trun_box,
    const pm_iso_box *moof,
    int32_t mdat_start,
    int32_t mdat_end,
    const pm_cmaf_tfhd *tfhd,
    uint32_t default_duration,
    bool has_default_duration,
    uint32_t default_size,
    bool has_default_size,
    uint32_t default_flags,
    bool has_default_flags,
    int64_t *decode_time,
    pm_cmaf_sample *out,
    int32_t out_capacity,
    int32_t *written,
    pm_cmaf_status *status
) {
    const int32_t body = pm_cmaf_payload_start(trun_box);
    const int32_t end = pm_cmaf_payload_end(trun_box);
    if (!pm_cmaf_fits(body, 8, end) || !pm_cmaf_fits(body, 8, length)) {
        pm_cmaf_fail(status, PM_CMAF_TRUNCATED, 0);
        return false;
    }
    const uint32_t flags = pm_iso_load_u24(data, body + 1);
    const int32_t sample_count = (int32_t)pm_iso_load_u32(data, body + 4);
    if (sample_count <= 0) {
        pm_cmaf_fail(status, PM_CMAF_MISSING_MDAT, 0);
        return false;
    }
    int32_t offset = body + 8;
    bool has_data_offset = false;
    int32_t data_offset = 0;
    bool has_first_flags = false;
    uint32_t first_flags = 0;
    if (flags & 0x000001) {
        if (!pm_cmaf_fits(offset, 4, end)) {
            pm_cmaf_fail(status, PM_CMAF_TRUNCATED, 0);
            return false;
        }
        data_offset = pm_iso_load_i32(data, offset);
        has_data_offset = true;
        offset += 4;
    }
    if (flags & 0x000004) {
        if (!pm_cmaf_fits(offset, 4, end)) {
            pm_cmaf_fail(status, PM_CMAF_TRUNCATED, 0);
            return false;
        }
        first_flags = pm_iso_load_u32(data, offset);
        has_first_flags = true;
        offset += 4;
    }

    int32_t row = 0;
    if (flags & 0x000100) {
        row += 4;
    }
    if (flags & 0x000200) {
        row += 4;
    }
    if (flags & 0x000400) {
        row += 4;
    }
    if (flags & 0x000800) {
        row += 4;
    }
    if (row > 0) {
        if (sample_count > (end - offset) / row) {
            pm_cmaf_fail(status, PM_CMAF_TRUNCATED, 0);
            return false;
        }
    }

    uint32_t first_size = 0;
    bool has_first_size = false;
    if (flags & 0x000200) {
        int32_t size_at = offset;
        if (flags & 0x000100) {
            size_at += 4;
        }
        first_size = pm_iso_load_u32(data, size_at);
        has_first_size = true;
    } else if (tfhd->has_size) {
        first_size = tfhd->default_size;
        has_first_size = true;
    } else if (has_default_size) {
        first_size = default_size;
        has_first_size = true;
    }

    const int32_t moof_based = moof->start + (has_data_offset ? data_offset : 0);
    const bool start_inside = moof_based >= mdat_start && moof_based < mdat_end;
    const bool first_fits = has_first_size
        && (int64_t)moof_based + (int64_t)first_size <= mdat_end;
    int32_t sample_offset = start_inside && first_fits ? moof_based : mdat_start;

    int32_t cursor = offset;
    for (int32_t index = 0; index < sample_count; index++) {
        uint32_t duration = 0;
        bool has_duration = false;
        uint32_t size = 0;
        bool has_size = false;
        uint32_t sample_flags = 0;
        bool has_flags = false;
        int32_t cts = 0;
        if (flags & 0x000100) {
            duration = pm_iso_load_u32(data, cursor);
            has_duration = true;
            cursor += 4;
        }
        if (flags & 0x000200) {
            size = pm_iso_load_u32(data, cursor);
            has_size = true;
            cursor += 4;
        }
        if (flags & 0x000400) {
            sample_flags = pm_iso_load_u32(data, cursor);
            has_flags = true;
            cursor += 4;
        }
        if (flags & 0x000800) {
            cts = pm_iso_load_i32(data, cursor);
            cursor += 4;
        }
        if (!has_duration) {
            if (tfhd->has_duration) {
                duration = tfhd->default_duration;
                has_duration = true;
            } else if (has_default_duration) {
                duration = default_duration;
                has_duration = true;
            }
        }
        if (!has_size) {
            if (tfhd->has_size) {
                size = tfhd->default_size;
                has_size = true;
            } else if (has_default_size) {
                size = default_size;
                has_size = true;
            }
        }
        if (!has_size || size == 0) {
            pm_cmaf_fail(status, PM_CMAF_MISSING_SIZE, 0);
            return false;
        }
        const int32_t start = sample_offset;
        if (size > (uint32_t)INT32_MAX
            || start < mdat_start
            || (int64_t)start + (int64_t)size > mdat_end) {
            pm_cmaf_fail(status, PM_CMAF_SAMPLE_OUTSIDE, 0);
            return false;
        }

        bool is_sync = true;
        if (index == 0 && has_first_flags) {
            is_sync = (first_flags & 0x00010000) == 0;
        } else if (has_flags) {
            is_sync = (sample_flags & 0x00010000) == 0;
        } else if (tfhd->has_flags) {
            is_sync = (tfhd->default_flags & 0x00010000) == 0;
        } else if (has_default_flags) {
            is_sync = (default_flags & 0x00010000) == 0;
        }

        pm_cmaf_sample sample;
        sample.offset = start;
        sample.size = (int32_t)size;
        sample.decode_time = *decode_time;
        sample.presentation_time = *decode_time + (int64_t)cts;
        sample.duration = has_duration ? (int64_t)duration : 0;
        sample.is_sync = is_sync;
        if (!pm_cmaf_store_sample(out, out_capacity, written, sample, status)) {
            return false;
        }
        *decode_time += sample.duration;
        sample_offset += (int32_t)size;
    }
    return true;
}

static bool pm_cmaf_extract_traf(
    const uint8_t *data,
    int32_t length,
    const pm_iso_box *traf,
    const pm_iso_box *moof,
    int32_t mdat_start,
    int32_t mdat_end,
    uint32_t track_id,
    uint32_t default_duration,
    bool has_default_duration,
    uint32_t default_size,
    bool has_default_size,
    uint32_t default_flags,
    bool has_default_flags,
    int64_t *decode_time,
    bool *has_decode_time,
    pm_cmaf_sample *out,
    int32_t out_capacity,
    int32_t *written,
    pm_cmaf_status *status
) {
    pm_iso_box children[PM_CMAF_BOX_CAP];
    const int32_t child_count = pm_cmaf_walk_children(
        data,
        length,
        traf,
        children,
        PM_CMAF_BOX_CAP,
        status
    );
    if (child_count < 0) {
        return false;
    }
    const pm_iso_box *tfhd_box = pm_cmaf_find_box(
        children,
        child_count,
        PM_ISO_TFHD
    );
    if (tfhd_box == NULL) {
        pm_cmaf_fail(status, PM_CMAF_MISSING_TFHD, 0);
        return false;
    }
    pm_cmaf_tfhd tfhd;
    if (!pm_cmaf_parse_tfhd(data, length, tfhd_box, &tfhd, status)) {
        return false;
    }
    if (tfhd.track_id != track_id) {
        pm_cmaf_fail(status, PM_CMAF_TRACK_MISMATCH, tfhd.track_id);
        return false;
    }

    const pm_iso_box *tfdt_box = pm_cmaf_find_box(
        children,
        child_count,
        PM_ISO_TFDT
    );
    if (tfdt_box != NULL) {
        if (!pm_cmaf_parse_tfdt(data, length, tfdt_box, decode_time, status)) {
            return false;
        }
        *has_decode_time = true;
    } else if (!*has_decode_time) {
        *decode_time = 0;
        *has_decode_time = true;
    }

    bool saw_trun = false;
    const int32_t before = *written;
    for (int32_t index = 0; index < child_count; index++) {
        if (children[index].type != PM_ISO_TRUN) {
            continue;
        }
        saw_trun = true;
        if (!pm_cmaf_extract_trun(
                data,
                length,
                &children[index],
                moof,
                mdat_start,
                mdat_end,
                &tfhd,
                default_duration,
                has_default_duration,
                default_size,
                has_default_size,
                default_flags,
                has_default_flags,
                decode_time,
                out,
                out_capacity,
                written,
                status
            )) {
            return false;
        }
    }
    if (!saw_trun || *written == before) {
        pm_cmaf_fail(status, PM_CMAF_MISSING_MDAT, 0);
        return false;
    }
    return true;
}

int32_t pm_cmaf_extract_fragment(
    const uint8_t *data,
    int32_t length,
    uint32_t track_id,
    uint32_t default_duration,
    bool has_default_duration,
    uint32_t default_size,
    bool has_default_size,
    uint32_t default_flags,
    bool has_default_flags,
    int64_t *decode_time,
    bool *has_decode_time,
    pm_cmaf_sample *out,
    int32_t out_capacity,
    pm_cmaf_status *status
) {
    pm_cmaf_status local = {PM_CMAF_OK, 0};
    pm_cmaf_status *st = status != NULL ? status : &local;
    st->code = PM_CMAF_OK;
    st->found_track_id = 0;
    if (data == NULL || length < 16 || decode_time == NULL
        || has_decode_time == NULL) {
        pm_cmaf_fail(st, PM_CMAF_MISSING_MOOF, 0);
        return 0;
    }

    pm_iso_box top[PM_CMAF_BOX_CAP];
    pm_iso_status walk = {PM_ISO_OK, 0, 0};
    const int32_t top_count = pm_iso_walk(
        data,
        length,
        0,
        length,
        top,
        PM_CMAF_BOX_CAP,
        &walk
    );
    const pm_iso_box *moof = pm_cmaf_find_box(top, top_count, PM_ISO_MOOF);
    const pm_iso_box *mdat = pm_cmaf_find_box(top, top_count, PM_ISO_MDAT);
    if (moof == NULL) {
        pm_cmaf_fail(st, PM_CMAF_MISSING_MOOF, 0);
        return 0;
    }
    if (mdat == NULL) {
        pm_cmaf_fail(st, PM_CMAF_MISSING_MDAT, 0);
        return 0;
    }

    pm_iso_box moof_children[PM_CMAF_BOX_CAP];
    const int32_t moof_count = pm_cmaf_walk_children(
        data,
        length,
        moof,
        moof_children,
        PM_CMAF_BOX_CAP,
        st
    );
    if (moof_count < 0) {
        return 0;
    }

    const int32_t mdat_start = pm_cmaf_payload_start(mdat);
    const int32_t mdat_end = pm_cmaf_payload_end(mdat);
    int32_t written = 0;
    int64_t running = *decode_time;
    bool has_running = *has_decode_time;
    bool saw_traf = false;
    for (int32_t index = 0; index < moof_count; index++) {
        if (moof_children[index].type != PM_ISO_TRAF) {
            continue;
        }
        saw_traf = true;
        if (!pm_cmaf_extract_traf(
                data,
                length,
                &moof_children[index],
                moof,
                mdat_start,
                mdat_end,
                track_id,
                default_duration,
                has_default_duration,
                default_size,
                has_default_size,
                default_flags,
                has_default_flags,
                &running,
                &has_running,
                out,
                out_capacity,
                &written,
                st
            )) {
            return 0;
        }
    }
    if (!saw_traf || written == 0) {
        pm_cmaf_fail(st, PM_CMAF_MISSING_MDAT, 0);
        return 0;
    }
    *decode_time = running;
    *has_decode_time = has_running;
    st->code = PM_CMAF_OK;
    return written;
}

#pragma mark - CMAF initialization parse

static int32_t pm_cmaf_walk_range(
    const uint8_t *data,
    int32_t length,
    int32_t start,
    int32_t end,
    pm_iso_box *out,
    int32_t out_capacity,
    pm_cmaf_status *status
) {
    pm_iso_status walk = {PM_ISO_OK, 0, 0};
    const int32_t count = pm_iso_walk(
        data,
        length,
        start,
        end,
        out,
        out_capacity,
        &walk
    );
    if (walk.code != PM_ISO_OK && count == 0) {
        pm_cmaf_fail(status, PM_CMAF_TRUNCATED, 0);
        return -1;
    }
    return count;
}

static int32_t pm_cmaf_stored_count(int32_t count) {
    if (count < 0) {
        return 0;
    }
    if (count > PM_CMAF_BOX_CAP) {
        return PM_CMAF_BOX_CAP;
    }
    return count;
}

static void pm_cmaf_init_clear(pm_cmaf_init *out) {
    if (out == NULL) {
        return;
    }
    memset(out, 0, sizeof(*out));
    out->esds_offset = -1;
    out->esds_length = 0;
    out->asc_offset = -1;
    out->asc_length = 0;
}

static bool pm_cmaf_find_asc(
    const uint8_t *data,
    int32_t start,
    int32_t end,
    int32_t depth,
    int32_t *asc_offset,
    int32_t *asc_length
) {
    if (data == NULL || depth > 8 || start < 0 || start > end) {
        return false;
    }
    int32_t offset = start;
    while (offset + 2 <= end) {
        const uint8_t tag = data[offset];
        offset += 1;
        uint32_t size = 0;
        int read = 0;
        while (read < 4) {
            if (offset >= end) {
                return false;
            }
            const uint8_t byte = data[offset];
            offset += 1;
            read += 1;
            size = (size << 7) | (uint32_t)(byte & 0x7F);
            if ((byte & 0x80) == 0) {
                break;
            }
        }
        if (size > (uint32_t)INT32_MAX) {
            return false;
        }
        const int32_t payload_size = (int32_t)size;
        if (!pm_cmaf_fits(offset, payload_size, end)) {
            break;
        }
        if (tag == 0x05) {
            if (asc_offset != NULL) {
                *asc_offset = offset;
            }
            if (asc_length != NULL) {
                *asc_length = payload_size;
            }
            return true;
        }
        const int32_t payload_end = offset + payload_size;
        int32_t nested_start = -1;
        if (tag == 0x03) {
            nested_start = offset + 3;
        } else if (tag == 0x04) {
            nested_start = offset + 13;
        }
        if (nested_start >= 0 && nested_start < payload_end) {
            if (pm_cmaf_find_asc(
                data,
                nested_start,
                payload_end,
                depth + 1,
                asc_offset,
                asc_length
            )) {
                return true;
            }
        }
        offset = payload_end;
    }
    return false;
}

static bool pm_cmaf_parse_tkhd_id(
    const uint8_t *data,
    int32_t length,
    const pm_iso_box *box,
    uint32_t *track_id
) {
    const int32_t body = pm_cmaf_payload_start(box);
    const int32_t end = pm_cmaf_payload_end(box);
    if (!pm_cmaf_fits(body, 1, end) || !pm_cmaf_fits(body, 1, length)) {
        return false;
    }
    const uint8_t version = data[body];
    const int32_t id_offset = version == 1 ? body + 20 : body + 12;
    if (!pm_cmaf_fits(id_offset, 4, end) || !pm_cmaf_fits(id_offset, 4, length)) {
        return false;
    }
    const uint32_t value = pm_iso_load_u32(data, id_offset);
    if (value == 0) {
        return false;
    }
    if (track_id != NULL) {
        *track_id = value;
    }
    return true;
}

static bool pm_cmaf_parse_mdhd_timescale(
    const uint8_t *data,
    int32_t length,
    const pm_iso_box *box,
    uint32_t *timescale
) {
    const int32_t body = pm_cmaf_payload_start(box);
    const int32_t end = pm_cmaf_payload_end(box);
    if (!pm_cmaf_fits(body, 5, end) || !pm_cmaf_fits(body, 5, length)) {
        return false;
    }
    const uint8_t version = data[body];
    const int32_t scale_offset = version == 1 ? body + 20 : body + 12;
    if (!pm_cmaf_fits(scale_offset, 4, end)
        || !pm_cmaf_fits(scale_offset, 4, length)) {
        return false;
    }
    const uint32_t value = pm_iso_load_u32(data, scale_offset);
    if (value == 0) {
        return false;
    }
    if (timescale != NULL) {
        *timescale = value;
    }
    return true;
}

static bool pm_cmaf_parse_hdlr_is_audio(
    const uint8_t *data,
    int32_t length,
    const pm_iso_box *box,
    bool *is_audio
) {
    const int32_t body = pm_cmaf_payload_start(box);
    const int32_t end = pm_cmaf_payload_end(box);
    if (!pm_cmaf_fits(body, 12, end) || !pm_cmaf_fits(body, 12, length)) {
        return false;
    }
    const uint32_t handler = pm_iso_load_u32(data, body + 8);
    if (is_audio != NULL) {
        *is_audio = handler == PM_ISO_SOUN;
    }
    return true;
}

static bool pm_cmaf_parse_audio_sample_entry(
    const uint8_t *data,
    int32_t length,
    const pm_iso_box *entry,
    uint32_t *channels,
    uint32_t *sample_rate
) {
    const int32_t body = pm_cmaf_payload_start(entry);
    const int32_t end = pm_cmaf_payload_end(entry);
    if (!pm_cmaf_fits(body, 28, end) || !pm_cmaf_fits(body, 28, length)) {
        return false;
    }
    const uint16_t channel_count = pm_iso_load_u16(data, body + 16);
    const uint32_t rate_fixed = pm_iso_load_u32(data, body + 24);
    if (channels != NULL) {
        *channels = channel_count;
    }
    if (sample_rate != NULL) {
        *sample_rate = rate_fixed >> 16;
    }
    return true;
}

static bool pm_cmaf_parse_esds(
    const uint8_t *data,
    int32_t length,
    const pm_iso_box *entry,
    pm_cmaf_init *out,
    pm_cmaf_status *status
) {
    const int32_t children_start = pm_cmaf_payload_start(entry) + 28;
    const int32_t children_end = pm_cmaf_payload_end(entry);
    if (children_start > children_end) {
        return true;
    }
    pm_iso_box nested[PM_CMAF_BOX_CAP];
    const int32_t nested_count = pm_cmaf_walk_range(
        data,
        length,
        children_start,
        children_end,
        nested,
        PM_CMAF_BOX_CAP,
        status
    );
    if (nested_count < 0) {
        return false;
    }
    const pm_iso_box *esds = pm_cmaf_find_box(
        nested,
        pm_cmaf_stored_count(nested_count),
        PM_ISO_ESDS
    );
    if (esds == NULL) {
        return true;
    }
    const int32_t descriptor_start = pm_cmaf_payload_start(esds) + 4;
    const int32_t descriptor_end = pm_cmaf_payload_end(esds);
    if (descriptor_start >= descriptor_end
        || !pm_cmaf_fits(descriptor_start, 0, length)) {
        return true;
    }
    out->esds_offset = descriptor_start;
    out->esds_length = descriptor_end - descriptor_start;
    int32_t asc_offset = -1;
    int32_t asc_length = 0;
    if (pm_cmaf_find_asc(
        data,
        descriptor_start,
        descriptor_end,
        0,
        &asc_offset,
        &asc_length
    )) {
        out->asc_offset = asc_offset;
        out->asc_length = asc_length;
    }
    return true;
}

static bool pm_cmaf_parse_trak(
    const uint8_t *data,
    int32_t length,
    const pm_iso_box *trak,
    pm_cmaf_init *out,
    bool *is_audio,
    pm_cmaf_status *status
) {
    pm_iso_box trak_children[PM_CMAF_BOX_CAP];
    const int32_t trak_count = pm_cmaf_walk_children(
        data,
        length,
        trak,
        trak_children,
        PM_CMAF_BOX_CAP,
        status
    );
    if (trak_count < 0) {
        return false;
    }
    const int32_t trak_stored = pm_cmaf_stored_count(trak_count);
    const pm_iso_box *tkhd = pm_cmaf_find_box(
        trak_children,
        trak_stored,
        PM_ISO_TKHD
    );
    const pm_iso_box *mdia = pm_cmaf_find_box(
        trak_children,
        trak_stored,
        PM_ISO_MDIA
    );
    if (tkhd == NULL || mdia == NULL) {
        return false;
    }

    pm_iso_box mdia_children[PM_CMAF_BOX_CAP];
    const int32_t mdia_count = pm_cmaf_walk_children(
        data,
        length,
        mdia,
        mdia_children,
        PM_CMAF_BOX_CAP,
        status
    );
    if (mdia_count < 0) {
        return false;
    }
    const int32_t mdia_stored = pm_cmaf_stored_count(mdia_count);
    const pm_iso_box *mdhd = pm_cmaf_find_box(
        mdia_children,
        mdia_stored,
        PM_ISO_MDHD
    );
    const pm_iso_box *hdlr = pm_cmaf_find_box(
        mdia_children,
        mdia_stored,
        PM_ISO_HDLR
    );
    const pm_iso_box *minf = pm_cmaf_find_box(
        mdia_children,
        mdia_stored,
        PM_ISO_MINF
    );
    if (mdhd == NULL || hdlr == NULL || minf == NULL) {
        return false;
    }

    pm_iso_box minf_children[PM_CMAF_BOX_CAP];
    const int32_t minf_count = pm_cmaf_walk_children(
        data,
        length,
        minf,
        minf_children,
        PM_CMAF_BOX_CAP,
        status
    );
    if (minf_count < 0) {
        return false;
    }
    const pm_iso_box *stbl = pm_cmaf_find_box(
        minf_children,
        pm_cmaf_stored_count(minf_count),
        PM_ISO_STBL
    );
    if (stbl == NULL) {
        return false;
    }

    pm_iso_box stbl_children[PM_CMAF_BOX_CAP];
    const int32_t stbl_count = pm_cmaf_walk_children(
        data,
        length,
        stbl,
        stbl_children,
        PM_CMAF_BOX_CAP,
        status
    );
    if (stbl_count < 0) {
        return false;
    }
    const pm_iso_box *stsd = pm_cmaf_find_box(
        stbl_children,
        pm_cmaf_stored_count(stbl_count),
        PM_ISO_STSD
    );
    if (stsd == NULL) {
        return false;
    }

    uint32_t track_id = 0;
    uint32_t timescale = 0;
    bool audio = false;
    if (!pm_cmaf_parse_tkhd_id(data, length, tkhd, &track_id)
        || !pm_cmaf_parse_mdhd_timescale(data, length, mdhd, &timescale)
        || !pm_cmaf_parse_hdlr_is_audio(data, length, hdlr, &audio)) {
        return false;
    }

    const int32_t entries_start = pm_cmaf_payload_start(stsd) + 8;
    const int32_t entries_end = pm_cmaf_payload_end(stsd);
    if (entries_start > entries_end) {
        return false;
    }
    pm_iso_box entries[PM_CMAF_BOX_CAP];
    const int32_t entry_count = pm_cmaf_walk_range(
        data,
        length,
        entries_start,
        entries_end,
        entries,
        PM_CMAF_BOX_CAP,
        status
    );
    if (entry_count < 0) {
        return false;
    }
    const int32_t entry_stored = pm_cmaf_stored_count(entry_count);
    if (entry_stored <= 0) {
        return false;
    }
    const pm_iso_box *entry = &entries[0];

    uint32_t channels = 0;
    uint32_t sample_rate = 0;
    (void)pm_cmaf_parse_audio_sample_entry(
        data,
        length,
        entry,
        &channels,
        &sample_rate
    );

    pm_cmaf_init_clear(out);
    out->track_id = track_id;
    out->timescale = timescale;
    out->sample_rate = sample_rate;
    out->channel_count = channels;
    out->codec_fourcc = entry->type;
    if (is_audio != NULL) {
        *is_audio = audio;
    }
    if (entry->type == PM_ISO_TYPE('m', 'p', '4', 'a')) {
        if (!pm_cmaf_parse_esds(data, length, entry, out, status)) {
            return false;
        }
    }
    return true;
}

static void pm_cmaf_fold_trex(
    const uint8_t *data,
    int32_t length,
    const pm_iso_box *moov,
    pm_cmaf_init *out,
    pm_cmaf_status *status
) {
    pm_iso_box moov_children[PM_CMAF_BOX_CAP];
    const int32_t moov_count = pm_cmaf_walk_children(
        data,
        length,
        moov,
        moov_children,
        PM_CMAF_BOX_CAP,
        status
    );
    if (moov_count < 0) {
        return;
    }
    const pm_iso_box *mvex = pm_cmaf_find_box(
        moov_children,
        pm_cmaf_stored_count(moov_count),
        PM_ISO_MVEX
    );
    if (mvex == NULL) {
        return;
    }
    pm_iso_box mvex_children[PM_CMAF_BOX_CAP];
    const int32_t mvex_count = pm_cmaf_walk_children(
        data,
        length,
        mvex,
        mvex_children,
        PM_CMAF_BOX_CAP,
        status
    );
    if (mvex_count < 0) {
        return;
    }
    const int32_t stored = pm_cmaf_stored_count(mvex_count);
    for (int32_t index = 0; index < stored; index++) {
        if (mvex_children[index].type != PM_ISO_TREX) {
            continue;
        }
        const int32_t body = pm_cmaf_payload_start(&mvex_children[index]);
        const int32_t end = pm_cmaf_payload_end(&mvex_children[index]);
        if (!pm_cmaf_fits(body, 24, end) || !pm_cmaf_fits(body, 24, length)) {
            continue;
        }
        const uint32_t track_id = pm_iso_load_u32(data, body + 4);
        if (track_id != out->track_id) {
            continue;
        }
        out->default_sample_duration = pm_iso_load_u32(data, body + 12);
        out->default_sample_size = pm_iso_load_u32(data, body + 16);
        out->default_sample_flags = pm_iso_load_u32(data, body + 20);
        out->has_default_duration = true;
        out->has_default_size = true;
        out->has_default_flags = true;
        return;
    }
}

int32_t pm_cmaf_parse_initialization(
    const uint8_t *data,
    int32_t length,
    pm_cmaf_init *out,
    pm_cmaf_status *status
) {
    pm_cmaf_status local = {PM_CMAF_OK, 0};
    pm_cmaf_status *st = status != NULL ? status : &local;
    st->code = PM_CMAF_OK;
    st->found_track_id = 0;
    if (out != NULL) {
        pm_cmaf_init_clear(out);
    }
    if (data == NULL || length < 8 || out == NULL) {
        pm_cmaf_fail(st, PM_CMAF_INVALID_INIT, 0);
        return 0;
    }

    pm_iso_box top[PM_CMAF_BOX_CAP];
    const int32_t top_count = pm_cmaf_walk_range(
        data,
        length,
        0,
        length,
        top,
        PM_CMAF_BOX_CAP,
        st
    );
    if (top_count < 0) {
        return 0;
    }
    const int32_t top_stored = pm_cmaf_stored_count(top_count);
    if (pm_cmaf_find_box(top, top_stored, PM_ISO_FTYP) == NULL) {
        pm_cmaf_fail(st, PM_CMAF_MISSING_FTYP, 0);
        return 0;
    }
    const pm_iso_box *moov = pm_cmaf_find_box(top, top_stored, PM_ISO_MOOV);
    if (moov == NULL) {
        pm_cmaf_fail(st, PM_CMAF_MISSING_MOOV, 0);
        return 0;
    }

    pm_iso_box moov_children[PM_CMAF_BOX_CAP];
    const int32_t moov_count = pm_cmaf_walk_children(
        data,
        length,
        moov,
        moov_children,
        PM_CMAF_BOX_CAP,
        st
    );
    if (moov_count < 0) {
        return 0;
    }
    const int32_t moov_stored = pm_cmaf_stored_count(moov_count);

    pm_cmaf_init selected;
    pm_cmaf_init fallback;
    bool has_selected = false;
    bool has_fallback = false;
    pm_cmaf_init_clear(&selected);
    pm_cmaf_init_clear(&fallback);

    for (int32_t index = 0; index < moov_stored; index++) {
        if (moov_children[index].type != PM_ISO_TRAK) {
            continue;
        }
        pm_cmaf_init candidate;
        bool is_audio = false;
        pm_cmaf_init_clear(&candidate);
        if (!pm_cmaf_parse_trak(
            data,
            length,
            &moov_children[index],
            &candidate,
            &is_audio,
            st
        )) {
            st->code = PM_CMAF_OK;
            st->found_track_id = 0;
            continue;
        }
        if (is_audio) {
            selected = candidate;
            has_selected = true;
            break;
        }
        if (!has_fallback) {
            fallback = candidate;
            has_fallback = true;
        }
    }

    if (!has_selected && !has_fallback) {
        pm_cmaf_fail(st, PM_CMAF_NO_AUDIO_TRACK, 0);
        return 0;
    }
    *out = has_selected ? selected : fallback;
    pm_cmaf_fold_trex(data, length, moov, out, st);
    st->code = PM_CMAF_OK;
    st->found_track_id = out->track_id;
    return 1;
}

#pragma mark - VK unmask

static const char pm_vk_alphabet[] =
    "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMN0PQRSTUVWXYZO123456789+/=";

static int pm_vk_alphabet_index(unsigned char character) {
    for (int index = 0; pm_vk_alphabet[index] != 0; index++) {
        if ((unsigned char)pm_vk_alphabet[index] == character) {
            return index;
        }
    }
    return -1;
}

static int32_t pm_vk_find(
    const uint8_t *data,
    int32_t length,
    const char *needle
) {
    if (data == NULL || needle == NULL) {
        return -1;
    }
    const int32_t needle_length = (int32_t)strlen(needle);
    if (needle_length <= 0 || needle_length > length) {
        return -1;
    }
    for (int32_t index = 0; index + needle_length <= length; index++) {
        if (memcmp(data + index, needle, (size_t)needle_length) == 0) {
            return index;
        }
    }
    return -1;
}

static int32_t pm_vk_decode(
    const uint8_t *value,
    int32_t length,
    uint8_t *out,
    int32_t out_capacity
) {
    int32_t written = 0;
    int accumulator = 0;
    int position = 0;
    for (int32_t index = 0; index < length; index++) {
        const int raw = pm_vk_alphabet_index(value[index]);
        if (raw < 0) {
            continue;
        }
        accumulator = position % 4 == 0 ? raw : 64 * accumulator + raw;
        const int previous = position;
        position += 1;
        if (previous % 4 == 0) {
            continue;
        }
        if (written >= out_capacity) {
            return PM_VK_UNMASK_OVERFLOW;
        }
        const int shift = (-2 * position) & 6;
        out[written] = (uint8_t)((accumulator >> shift) & 255);
        written += 1;
    }
    return written == 0 ? PM_VK_UNMASK_FAILED : written;
}

int32_t pm_vk_unmask(
    const uint8_t *raw,
    int32_t raw_length,
    int32_t user_id,
    uint8_t *out,
    int32_t out_capacity
) {
    if (raw == NULL || raw_length <= 0) {
        return PM_VK_UNMASK_FAILED;
    }
    if (pm_vk_find(raw, raw_length, "audio_api_unavailable") < 0) {
        return PM_VK_UNMASK_NOT_MASKED;
    }
    const int32_t extra = pm_vk_find(raw, raw_length, "?extra=");
    if (extra < 0) {
        return PM_VK_UNMASK_FAILED;
    }
    const int32_t encoded_start = extra + 7;
    if (encoded_start >= raw_length) {
        return PM_VK_UNMASK_FAILED;
    }
    int32_t hash = -1;
    for (int32_t index = encoded_start; index < raw_length; index++) {
        if (raw[index] == '#') {
            hash = index;
            break;
        }
    }
    if (hash < 0 || hash + 1 >= raw_length) {
        return PM_VK_UNMASK_FAILED;
    }

    uint8_t url_bytes[4096];
    uint8_t key_bytes[256];
    const int32_t url_decoded = pm_vk_decode(
        raw + encoded_start,
        hash - encoded_start,
        url_bytes,
        4096
    );
    if (url_decoded <= 0) {
        return PM_VK_UNMASK_FAILED;
    }
    const int32_t key_decoded = pm_vk_decode(
        raw + hash + 1,
        raw_length - (hash + 1),
        key_bytes,
        256
    );
    if (key_decoded <= 0) {
        return PM_VK_UNMASK_FAILED;
    }

    int32_t tab = -1;
    for (int32_t index = 0; index < key_decoded; index++) {
        if (key_bytes[index] == 0x0B) {
            tab = index;
            break;
        }
    }
    if (tab < 0 || tab + 1 >= key_decoded) {
        return PM_VK_UNMASK_FAILED;
    }
    int64_t parsed_key = 0;
    bool have_digit = false;
    for (int32_t index = tab + 1; index < key_decoded; index++) {
        const uint8_t byte = key_bytes[index];
        if (byte == 0x0B) {
            break;
        }
        if (byte < '0' || byte > '9') {
            if (!have_digit) {
                return PM_VK_UNMASK_FAILED;
            }
            break;
        }
        have_digit = true;
        parsed_key = parsed_key * 10 + (byte - '0');
        if (parsed_key > INT32_MAX) {
            return PM_VK_UNMASK_FAILED;
        }
    }
    if (!have_digit) {
        return PM_VK_UNMASK_FAILED;
    }

    if (url_decoded > out_capacity) {
        return PM_VK_UNMASK_OVERFLOW;
    }
    memcpy(out, url_bytes, (size_t)url_decoded);
    const int32_t character_length = url_decoded;
    int32_t state = (int32_t)parsed_key ^ user_id;
    // Reject oversized URLs before the fixed-size scratch buffer exists so
    // the allocation never sits ahead of its own bounds check.
    if (character_length > 4096) {
        return PM_VK_UNMASK_OVERFLOW;
    }
    int32_t indexes[4096];
    for (int32_t position = character_length - 1; position >= 0; position--) {
        state = ((character_length * (position + 1)) ^ (state + position))
            % character_length;
        if (state < 0) {
            state += character_length;
        }
        indexes[position] = state;
    }
    if (character_length > 1) {
        for (int32_t position = 1; position < character_length; position++) {
            const int32_t target = indexes[character_length - 1 - position];
            const uint8_t swap = out[position];
            out[position] = out[target];
            out[target] = swap;
        }
    }
    return character_length;
}

#pragma mark - Loaded-ahead fold

double pm_buffer_max_loaded_ahead(
    double position_seconds,
    const double *end_seconds,
    int32_t count
) {
    if (end_seconds == NULL || count <= 0 || !isfinite(position_seconds)) {
        return 0.0;
    }
    double loaded_ahead = 0.0;
    for (int32_t index = 0; index < count; index++) {
        const double end = end_seconds[index];
        if (!isfinite(end)) {
            continue;
        }
        const double ahead = end - position_seconds;
        if (isfinite(ahead) && ahead > loaded_ahead) {
            loaded_ahead = ahead;
        }
    }
    if (loaded_ahead < 0.0) {
        return 0.0;
    }
    return loaded_ahead;
}

#pragma mark - HLS AES-128 CBC

int32_t pm_aes128_cbc_decrypt(
    const uint8_t *ciphertext,
    int32_t ciphertext_length,
    const uint8_t *key,
    const uint8_t *iv,
    uint8_t *out,
    int32_t out_capacity,
    int32_t *out_length
) {
    if (out_length == NULL) {
        return PM_AES128_INVALID_ARGUMENT;
    }
    *out_length = 0;
    if (ciphertext == NULL || key == NULL || iv == NULL || out == NULL) {
        return PM_AES128_INVALID_ARGUMENT;
    }
    if (ciphertext_length <= 0
        || (ciphertext_length % PM_AES128_BLOCK_BYTES) != 0
        || out_capacity < ciphertext_length + PM_AES128_BLOCK_BYTES) {
        return PM_AES128_OUTPUT_TOO_SMALL;
    }

    size_t plaintext_length = 0;
    const CCCryptorStatus status = CCCrypt(
        kCCDecrypt,
        kCCAlgorithmAES,
        kCCOptionPKCS7Padding,
        key,
        PM_AES128_KEY_BYTES,
        iv,
        ciphertext,
        (size_t)ciphertext_length,
        out,
        (size_t)out_capacity,
        &plaintext_length
    );
    if (status != kCCSuccess) {
        return PM_AES128_DECRYPT_FAILED;
    }
    if (plaintext_length > (size_t)out_capacity) {
        return PM_AES128_OUTPUT_TOO_SMALL;
    }
    *out_length = (int32_t)plaintext_length;
    return PM_AES128_OK;
}
