#!/usr/bin/env python3
"""Predict a name for each unnamed opcode by position, using the validated Kinoko ordering alignment.

  scripts/align-kinoko.py [--max-span=N]

Our opcode numbers are far newer than any public table, so the NUMBERS do not map. The ORDER does: aligning
Kinoko's OutHeader table (which mirrors the client's own internal handler naming) against our already-named
opcodes preserves ordering across all 13 anchors, several of which were named here independently before the
table was ever consulted (e.g. ShopSign -> UserADBoard). See docs/decode-plan.md.

So: for each unnamed opcode, find the two named anchors it sits between, and report the Kinoko entries that lie
between those anchors' Kinoko values. That is the candidate set. A candidate set of exactly one is a strong
prediction; a wide set is only a shortlist.

This is a HYPOTHESIS GENERATOR, not an authority: every prediction still has to be checked against a real
payload shape before any name is written (that is how 0x022e/0x02b9/0x02bb were accepted and how 0x02af's
adjacency test was run). Report only - writes nothing.
"""
import re, sys

MAX_SPAN = int(next((a.split("=")[1] for a in sys.argv if a.startswith("--max-span=")), 6))

# Kinoko OutHeader, values 170-340 (the CUserPool/CMobPool/CNpcPool/CDropPool span our unknowns fall in).
KINOKO = {
    170:"DestroyClock",171:"ShowArenaResult",172:"StalkResult",173:"MassacreIncGauge",174:"MassacreResult",
    175:"QuickslotMappedInit",176:"FootHoldInfo",177:"RequestFootHoldInfo",178:"FieldKillCount",
    179:"UserEnterField",180:"UserLeaveField",181:"UserChat",182:"UserChatNLCPQ",183:"UserADBoard",
    184:"UserMiniRoomBalloon",185:"UserConsumeItemEffect",186:"UserItemUpgradeEffect",
    187:"UserItemHyperUpgradeEffect",188:"UserItemOptionUpgradeEffect",189:"UserItemReleaseEffect",
    190:"UserItemUnreleaseEffect",191:"UserHitByUser",192:"UserTeslaTriangle",193:"UserFollowCharacter",
    194:"UserShowPQReward",195:"UserSetPhase",196:"SetPortalUsable",197:"ShowPamsSongResult",
    198:"PetActivated",199:"PetEvol",200:"PetTransferField",201:"PetMove",202:"PetAction",
    203:"PetNameChanged",204:"PetLoadExceptionList",205:"PetActionCommand",206:"DragonEnterField",
    207:"DragonMove",208:"DragonLeaveField",210:"UserMove",211:"UserMeleeAttack",212:"UserShootAttack",
    213:"UserMagicAttack",214:"UserBodyAttack",215:"UserSkillPrepare",216:"UserMovingShootAttackPrepare",
    217:"UserSkillCancel",218:"UserHit",219:"UserEmotion",220:"UserSetActiveEffectItem",
    221:"UserShowUpgradeTombEffect",222:"UserSetActivePortableChair",223:"UserAvatarModified",
    224:"UserEffectRemote",225:"UserTemporaryStatSet",226:"UserTemporaryStatReset",227:"UserHP",
    228:"UserGuildNameChanged",229:"UserGuildMarkChanged",230:"UserThrowGrenade",231:"UserSitResult",
    232:"UserEmotionLocal",233:"UserEffectLocal",234:"UserTeleport",235:"Premium",236:"MesoGive_Succeeded",
    237:"MesoGive_Failed",238:"Random_Mesobag_Succeed",239:"Random_Mesobag_Failed",240:"FieldFadeInOut",
    241:"FieldFadeOutForce",242:"UserQuestResult",243:"NotifyHPDecByField",244:"UserPetSkillChanged",
    245:"UserBalloonMsg",246:"PlayEventSound",247:"PlayMinigameSound",248:"UserMakerResult",
    249:"UserOpenConsultBoard",250:"UserOpenClassCompetitionPage",251:"UserOpenUI",252:"UserOpenUIWithOption",
    253:"SetDirectionMode",254:"SetStandAloneMode",255:"UserHireTutor",256:"UserTutorMsg",257:"IncCombo",
    258:"UserRandomEmotion",259:"ResignQuestReturn",260:"PassMateName",261:"SetRadioSchedule",
    262:"UserOpenSkillGuide",263:"UserNoticeMsg",264:"UserChatMsg",265:"UserBuffzoneEffect",
    266:"UserGoToCommoditySN",267:"UserDamageMeter",268:"UserTimeBombAttack",269:"UserPassiveMove",
    270:"UserFollowCharacterFailed",271:"UserRequestVengeance",272:"UserRequestExJablin",
    273:"UserAskAPSPEvent",274:"QuestGuideResult",275:"UserDeliveryQuest",276:"SkillCooltimeSet",
    278:"SummonedEnterField",279:"SummonedLeaveField",280:"SummonedMove",281:"SummonedAttack",
    282:"SummonedSkill",283:"SummonedHit",284:"MobEnterField",285:"MobLeaveField",286:"MobChangeController",
    287:"MobMove",288:"MobCtrlAck",289:"MobCtrlHint",290:"MobStatSet",291:"MobStatReset",
    292:"MobSuspendReset",293:"MobAffected",294:"MobDamaged",295:"MobSpecialEffectBySkill",
    296:"MobHPChange",297:"MobCrcKeyChanged",298:"MobHPIndicator",299:"MobCatchEffect",
    300:"MobEffectByItem",301:"MobSpeaking",302:"MobChargeCount",303:"MobSkillDelay",
    304:"MobRequestResultEscortInfo",305:"MobEscortStopEndPermmision",306:"MobEscortStopSay",
    307:"MobEscortReturnBefore",308:"MobNextAttack",309:"MobAttackedByMob",311:"NpcEnterField",
    312:"NpcLeaveField",313:"NpcChangeController",314:"NpcMove",315:"NpcUpdateLimitedInfo",
    316:"NpcSpecialAction",317:"NpcSetScript",319:"EmployeeEnterField",320:"EmployeeLeaveField",
    321:"EmployeeMiniRoomBalloon",322:"DropEnterField",323:"DropReleaseAllFreeze",324:"DropLeaveField",
    325:"CreateMessgaeBoxFailed",326:"MessageBoxEnterField",327:"MessageBoxLeaveField",
    328:"AffectedAreaCreated",329:"AffectedAreaRemoved",330:"TownPortalCreated",331:"TownPortalRemoved",
    332:"OpenGateCreated",333:"OpenGateRemoved",334:"ReactorChangeState",335:"ReactorMove",
    336:"ReactorEnterField",337:"ReactorLeaveField",338:"SnowBallState",339:"SnowBallHit",340:"SnowBallMsg",
}

# Anchors: (our opcode, Kinoko value). Only ones whose identification is solid.
ANCHORS = [(0x022d,179),(0x023d,183),(0x0282,201),(0x029d,210),(0x02a8,211),(0x02b8,223),
           (0x03c9,284),(0x03d4,285),(0x03d5,286),(0x03dc,287),(0x0456,314),(0x0471,322),(0x0487,336)]

# Accepted hypotheses (position + an independent structural or behavioural check - see each opcode's summary).
# Using them as anchors is the compounding step that makes this method systematic: every opcode that gets
# confirmed narrows the candidate sets of its neighbours. Kept behind a flag so the conservative view, which
# relies only on long-confirmed names, stays available.
HYPOTHESIS_ANCHORS = [(0x022e,180),(0x02af,218),(0x02b9,224),(0x02bb,225)]
if "--with-hypotheses" in sys.argv:
    ANCHORS = ANCHORS + HYPOTHESIS_ANCHORS

text = open("neon-protocol/opcodes.toml").read()
entries = {}
for b in re.split(r"(?=\[\[opcode\]\])", text):
    i = re.search(r'^id = (0x[0-9a-f]+)', b, re.M)
    d = re.search(r'^dir = "(\w+)"', b, re.M)
    if not (i and d and d[1] == "in"):
        continue
    oid = int(i[1], 16)
    named = re.search(r'^name = "([^"]+)"', b, re.M)
    cnt = re.search(r'FACTS \(exact-decoded logs, [0-9-]+\): (\d+) packets', b)
    prev = entries.get(oid)
    entries[oid] = (named[1] if named else (prev[0] if prev else None),
                    int(cnt[1]) if cnt else (prev[1] if prev else 0),
                    "HYPOTHESIS" in b or (prev[2] if prev else False))

ANCHORS.sort()
unnamed = sorted([(o, v[1], v[2]) for o, v in entries.items() if not v[0]], key=lambda r: -r[1])

print(f"{len(ANCHORS)} anchors; predicting for {len(unnamed)} unnamed incoming opcodes "
      f"(candidate sets wider than {MAX_SPAN} are shown as a count only)\n")
print(f"{'opcode':>8}  {'packets':>8}  candidates (Kinoko entries between the surrounding anchors)")
hits = 0
for oid, cnt, has_hyp in unnamed:
    lo = max([a for a in ANCHORS if a[0] < oid], default=None)
    hi = min([a for a in ANCHORS if a[0] > oid], default=None)
    if not lo or not hi:
        continue
    span = [KINOKO[k] for k in sorted(KINOKO) if lo[1] < k < hi[1]]
    if not span:
        continue
    mark = " *" if has_hyp else ""
    if len(span) <= MAX_SPAN:
        hits += 1
        print(f"{oid:>#8x}  {cnt:>8}  {', '.join(span)}{mark}")
    else:
        print(f"{oid:>#8x}  {cnt:>8}  [{len(span)} candidates between {KINOKO[lo[1]]} and {KINOKO[hi[1]]}]{mark}")
print(f"\n{hits} opcode(s) have a candidate set of <= {MAX_SPAN}. '*' = a hypothesis is already recorded.")
print("Every candidate still needs its payload shape checked against real captures before being named.")
