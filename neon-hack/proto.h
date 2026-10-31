/* Shared contract between neon-hack.dll (inside the game) and the panel (outside).
 * One mapping per pid: Local\NeonHack.<pid>, NEON_HACK_BYTES long.
 * The panel creates nothing; it opens what the DLL made and writes cmd/force_*.
 */
#pragma once
#include <windows.h>

#define NEON_HACK_MAGIC 0x4E454F48u /* 'HOEN' */
#define NEON_HACK_VERSION 1u

enum {
    HACK_CMD_OFF = 0,
    HACK_CMD_ON = 1,      /* scan, then freeze confirmed addresses */
    HACK_CMD_FORCE = 2,   /* lock force_addr = force_value every tick */
};

enum {
    HACK_STATE_IDLE = 0,
    HACK_STATE_SCANNING = 1,
    HACK_STATE_CONFIRMED = 2,
    HACK_STATE_FROZEN = 3,
};

typedef struct _NEON_HACK_STATE {
    volatile ULONG magic;
    volatile ULONG version;
    volatile ULONG pid;
    volatile ULONGLONG module_base;
    volatile ULONG cmd;
    volatile ULONG state;
    volatile ULONG candidates;
    volatile ULONG damage_events;
    volatile ULONG hp;
    volatile ULONG max_hp;
    volatile ULONG frozen_count;
    volatile ULONG last_error;
    volatile ULONG ticks;
    volatile ULONGLONG frozen_addr[4];
    volatile ULONG frozen_value[4];
    volatile ULONGLONG force_addr;
    volatile ULONG force_value;
} NEON_HACK_STATE;

#define NEON_HACK_BYTES (sizeof(NEON_HACK_STATE) + 0x1000)
