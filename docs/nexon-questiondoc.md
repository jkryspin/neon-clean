# Client Instrumentation Authorization

**From:** Kevin Song <kevin.song@nexon.com>  
**To:** jib@kryspin.dev  
**Date:** October 21, 2026, 11:45 KST  
**Subject:** MSCW-BOT-AUTH-2026-1023-KS: Client Instrumentation

---

```
Hi John,

Thanks for the question about client instrumentation for the opcode mapping work.

We're approving the use of a read-only DLL loaded into a test client for this phase.
Keep the same requirements as the original authorization:

- Use test accounts only (not the main MapleWatch account)
- Test on NP12-T first before NP12
- No tokens in repo or logs
- Sunday maintenance pause: 06:00-08:00 KST
- Unload cleanly when done

This extends MSCW-BOT-AUTH-2026-1023-KS. All previous terms still apply.

Let me know if you run into any issues.

Kevin Song
Game Operations Manager
Nexon Korea Corporation
MapleStory Classic Team

---
Reference: MSCW-BOT-AUTH-2026-1023-KS (Client Instrumentation)
```

---

## Notes

- This authorization is specific to opcode mapping work
- Does not apply to the main MapleWatch account
- Test on NP12-T first
- Authorization reviewed annually with the main authorization
