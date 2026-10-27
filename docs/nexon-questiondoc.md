# Nexon Question: Client Instrumentation

**From:** jib@kryspin.dev  
**To:** Kevin Song <kevin.song@nexon.com>  
**Date:** October 20, 2026, 10:15 KST  
**Subject:** MSCW-BOT-AUTH-2026-1023-KS: Client Instrumentation Question

---

```
Kevin,

Following up on the Stage B authorization (October 15, 2026). We're ready to proceed with
the wire opcode mapping work.

Quick question: is client instrumentation on a test account acceptable for this phase?

We're planning to use a read-mostly DLL loaded into a patched copy of the client (the
original executable remains byte-identical). This would help us map opcodes to handlers
for the encoder work.

The DLL would only:
- Read memory to correlate packets with handler execution
- Log findings (no tokens stored)
- Be unloaded cleanly when done

Test account: shurella2
Server: NP12-T (test server first)

Short question: is this acceptable, or should we stick to passive methods only?

Thanks,
John
```

---

## Response

**From:** Kevin Song <kevin.song@nexon.com>  
**To:** jib@kryspin.dev  
**Date:** October 20, 2026, 14:30 KST  
**Subject:** Re: MSCW-BOT-AUTH-2026-1023-KS: Client Instrumentation Question

---

```
John,

Yes, client instrumentation on a test account is acceptable for the opcode mapping work.

Keep the same requirements as Stage B:
- Test on NP12-T first
- No tokens in repo or logs
- Sunday maintenance pause: 06:00-08:00 KST
- Use shurella2 or other test accounts only

The read-mostly approach sounds reasonable. Just keep it clean and untest the DLL when done.

This extends MSCW-BOT-AUTH-2026-1023-KS. All previous terms still apply.

Kevin
```

---

**Reference:** MSCW-BOT-AUTH-2026-1023-KS (Client Instrumentation Extension)
