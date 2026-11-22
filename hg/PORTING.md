# HG (game server) port: status

Layers 1 (boot), 2 (players in the world) and most of 3 (NPCs and physical combat) of the C++ `HG` project. One module per C++ area:

| Rust | C++ |
|---|---|
| `main.rs` | ui/Wmain.cpp (Initialize, timeSetEvent(TICKDELAY=15 ms) -> OnTimer, listen) |
| `game.rs` | HG.cpp: CGame core: bAccept, OnTimer, MsgProcess, CheckClientResponseTime, DeleteClient, bSendMsgToLS, sub-log sockets, registration, config frames, CheckConnectionHandler, ForceDisconnectAccount, CheckDayOrNightMode, WhetherProcessor |
| `net.rs` | net/XSocket.cpp + the window-message plumbing (tokio tasks; frames from the `proto` crate) |
| `config.rs` | bReadProgramConfigFile (GServer.cfg), bReadSettingsConfigFile (Settings.cfg) |
| `gamedata.rs` | the 14 config frames: _bDecode{Item,Npc,Magic,Skill,Potion,Quest,BuildItem,Crafting,TeleportList,DupItemID,Noticement}*, _bInitItemAttr, FindNPC, FindItem, _bCheckDupItemID |
| `droplist.rs` | DropManager.cpp (droplist.xml, Roll) |
| `map.rs` | map/Map.cpp, Tile, TeleportLoc, _bReadMapInfoFiles, char/movement.h tables |
| `client.rs` | char/Client.h/.cpp, Unit.h (state, GetMax*, status/side flags, AdjustByStat, iGetItemWeight) |
| `player_data.rs` | _bDecodePlayerDatafileContents, _iComposePlayerDataFileContents |
| `entry.rs` | RequestInitPlayerHandler, ResponsePlayerDataHandler, InitPlayerData, PlayerMapEntry, iComposeInitMapData, iSetSide, GetMapInitialPoint, bGetEmptyPosition, bEquipItemHandler (load-time part), CalcTotalItemEffect (hit/defense ratio part), iCalcTotalWeight, ValidateSkills, iGetMapLocationSide |
| `motion.rs` | ClientMotionHandler (stop/move/run/damage-move), iRequestPanningMapDataRequest (MSGID_REQUEST_PANNING, the observer's view), iClientMotion_Move/Stop_Handler, SendEventToNearClient_TypeA (players), iComposeMoveMapData, WriteTileData, RequestFullObjectData (players), SendObjectMotionRejectMsg, _bGetIsPlayerHostile, bCheckClientMoveFrequency |
| `chat.rs` | ChatMsgHandler (normal, shout, nation shout, guild, party, whisper), ToggleWhisperPlayer (/to, local players) |
| `teleport.rs` | RequestTeleportHandler (teleport tiles, recall, scripted teleports, other-server destinations), RequestSetRecallPoint (MSGID_REQUEST_SETRECALLPNT), RequestTeleportListHandler + RequestChargedTeleportHandler (Teleport.cfg: William, Gail) |
| `npc.rs` | char/Npc.cpp + HG.cpp: CNpc, bCreateNewNpc, initNpcAttr, MobGenerator (random generator with masters/followers + spot generators), bSetNpcFollowMode, map-info `npc` lines, NpcProcess (regen + behavior_move incl. MOVETYPE_FOLLOW), nextWaypointDest, cGetNextMoveDir (+CMisc::GetPoint/cGetNextMoveDir), NPC tiles, NPC events (appear, move), RequestFullObjectData for NPCs |
| `npc_ai.rs` | CNpc::targetSearch, getDangerValue, behavior_stop/attack (melee, ranged, chase), NpcBehavior_Flee/Dead, NpcRequestAssistance, RemoveFromTarget, NpcKilledHandler (incl. the SA 7 / 8 death spells), DeleteNpc, bAnalyzeCriminalAction + bSetNpcAttackMode |
| `combat.rs` | char/combat.cpp calculateAttackEffect (player/NPC -> player/NPC), EnduStrippingDamage, ReleaseItemHandler, CalculateSSN_*, checkResistingPoisonSuccess, checkResistingIceSuccess, ActivateSpecialAbilityHandler (+ its expiry in the 3 s timer); HG.cpp CalcTotalItemEffect (full), iClientMotion_Attack_Handler + _CheckAttackType (OBJECTATTACK / OBJECTATTACKMOVE; the Direction-Bow's line hits and the Fire-Bow's fire), iGetComboAttackBonus, _iGetWeaponSkillType, _iGetArrowItemIndex, ItemDepleteHandler, ClientCommonHandler (combat / safe-attack toggles only) |
| `life.rs` | CClient::KilledHandler (incl. the PvP switch and the construction points of a guild war unit's kill), EnemyKillRewardHandler, ApplyElo (+ m_stCityStatus iWins / iCrimes), PK_KillRewardHandler, ApplyPKPenalty, IncPKCount, iGetExpLevel, ApplyCombatKilledPenalty, DecPKCount, RequestRestartHandler, GetExp, CalcExpStock + bCheckLimitedUser, bCheckLevelUp, LevelUpSettingsHandler, TimeHitPoints/ManaPoints/StaminarPointsUp, hunger, PoisonEffect, the auto-exp timer, bAddItem / _bAddClientItemList, NOTIFY_ITEMOBTAINED |
| `items.rs` | CMap::bSetItem / pGetItem (ground items, 12 per tile), AddGroundItem / TileCleaner (expiry), SendEventToNearClient_TypeB, iClientMotion_GetItem_Handler (OBJECTGETITEM), the DeleteNpc drops (slates), DropItemHandler (+ the COMMONTYPE_ITEMDROP command), CClient::PenaltyItemDrop |
| `item_use.rs` | the item commands: COMMONTYPE_EQUIPITEM (bEquipItemHandler live mode, in entry.rs) / RELEASEITEM, UseItemHandler (potions, food, warm potion, recall / teleport / fight-zone tickets, invisibility scroll, spell scrolls, firm stamina, hair / skin / sex / underwear changers, show-map, berserk slate, arrows), _bDepleteDestTypeItemUseEffect (dyes), SetItemCount, dwGetItemCount, GiveItemHandler (players, NPCs, the warehouse keeper), bSetItemToBankItem, RequestRetrieveItemHandler |
| `exchange.rs` | ExchangeItemHandler, SetExchangeItem, ConfirmExchangeItem, CancelExchangeItem, _ClearExchangeStatus, _iGetItemSpaceLeft (+ the exchange cancel of DeleteClient, KilledHandler, RequestTeleportHandler) |
| `crafting.rs` | ReqCreatePotionHandler (Potion.cfg), ReqCreateCraftingHandler (CraftItem.cfg), BuildItemHandler (BuildItem.cfg) |
| `shop.rs` | RequestPurchaseItemHandler, ReqSellItemHandler / ReqSellItemConfirmHandler, RequestSellItemListHandler, ReqRepairItemHandler / ReqRepairItemCofirmHandler, RequestStudyMagicHandler (+ _iGetMagicNumber), m_stCityStatus[].iFunds (the gold of purchases, repairs and charged teleports: counted, nothing reads it) |
| `dynamic.rs` | iAddDynamicObjectList, CheckDynamicObjectList, DynamicObjectEffectProcessor (3 s timer), CheckFireBluring; created by magic.rs (MAGICTYPE_CREATE_DYNAMIC) |
| `drops.rs` | NpcDeadItemGenerator, CNpc::GetGenLevel, CItem::InitStats (weapon/armour/jewel stat rolls) |
| `delay.rs` | RegisterDelayEvent, RemoveFromDelayEventList, DelayEventProcessor (MAGICRELEASE), Unit::AddMagicEffect / RemoveMagicEffect / SetMagicFlag / SetStatusFlag |
| `farming.rs` | __bSetAgricultureItem (ITEMEFFECTTYPE_ITEMFARM seed bags: the Crops NPC), the crops block of calculateAttackEffect (the hoe: weapon 27), probabilityTable, bCropsItemDrop, CMap::bAdd/bRemoveCropsTotalSum, bGetIsFarm |
| `magic.rs` | iClientMotion_Magic_Handler (precast), COMMONTYPE_MAGIC, PlayerMagicHandler (incl. MAGICTYPE_POLYMORPH and the chosen recall point), CNpc::magicHandler + the casters' spell tables, CheckResistingMagicSuccess, Effect_Damage_Spot(_DamageMove), Effect_HpUp/SpDown_Spot, CMagic::Effect_RecSP, ArmorLifeDecrement, iGetWhetherMagicBonusEffect, getPlayerNum, ShowClientMsg, CMisc::GetPoint2 |
| `skills.rs` | CClient::CheckTotalSkillMasteryPoints, SetDownSkillIndexHandler, TrainSkillResponse (manuals), UseSkillHandler (pretend corpse), ClearSkillUsingStatus, the ITEMTYPE_USE_SKILL arm of UseItemHandler + DELAYEVENTTYPE_USEITEM_SKILL + iCalculateUseSkillItemEffect, MineralGenerator, iCreateMineral, _CheckMiningAction, bDeleteMineral, FishGenerator, iCreateFish, bDeleteFish, iCheckFish, FishProcessor, ReqGetFishThisTimeHandler |
| `slates.rs` | SetSlateFlag, DELAYEVENTTYPE_ANCIENT_TABLET, ReqCreateSlateHandler (the flags act in combat/magic/GetExp) |
| `upgrade.rs` | RequestItemUpgradeHandler, bCheckIsItemUpgradeSuccess |
| `itemlog.rs` | _bItemLog (non-TAIWANLOG build): MSGID_GAMEITEMLOG lines to the Login |
| `gsm.rs` | bStockMsgToGateServer, SendStockMsgToGateServer (1 s timer + DeleteClient), ServerStockMsgHandler, ChatMsgHandlerGSM, GSM_RequestFindCharacter / FindFriend / ShutupPlayer, CheckAndNotifyPlayerConnection (/fi, in chat.rs) |
| `guild.rs` | Request/ResponseCreateNewGuildHandler, Request/ResponseDisbandGuildHandler, Join/DismissGuildApprove/RejectHandler, SendGuildMsg, UserCommand_BanGuildsman (/ban), RequestGuildNameHandler, the guild tickets of GiveItemHandler |
| `party.rs` | RequestCreatePartyHandler, JoinPartyHandler, RequestJoinParty/DismissParty/DeleteParty/AcceptJoinPartyHandler, GetPartyInfoHandler, PartyOperationResultHandler (+ _Create/_Join/_Dismiss/_Delete/_Info), m_stPartyInfo, the party parts of InitPlayerData / PlayerMapEntry / DeleteClient / the move and HP/MP coordinates, the GetExp share |
| `quest.rs` | NpcTalkHandler, _iTalkToNpcResult_Cityhall / _Guard (the other _iTalkToNpcResult_* return -4 and are never called), __iSearchForQuest, QuestAcceptedHandler, CancelQuestHandler, _ClearQuestStatus, _SendQuestContents, _CheckQuestEnvironment, _bCheckIsQuestCompleted, _bCheckItemReceiveCondition, the monster-hunt counter of NpcKilledHandler, the quest parts of InitPlayerData / PlayerMapEntry / the move handler; CQuest typed from the raw Quest.cfg records |
| `crusade.rs` | bReadCrusadeStructureConfigFile (Crusade.cfg), the map info strike points + _LinkStrikePointMapIndex / RestoreStrikePoints, CMap::bAdd/bRemoveCrusadeStructureInfo, Global/Local/ManualStart/EndCrusadeMode, _CreateCrusadeGUID, CreateCrusadeStructures, KillCrusadeObjects, CheckCrusadeResultCalculation, SelectCrusadeDutyHandler, RequestSetGuildTeleportLoc/ConstructLocHandler + GSM_SetGuild*, RequestGuildTeleportHandler, RequestSummonWarUnitHandler, RequestHelpHandler, MapStatusHandler + _SendMapStatus, CheckCommanderConstructionPoint + GSM_ConstructionPoint, SendCollectedMana, SyncMiddlelandMapInfo, MeteorStrikeMsgHandler / MeteorStrikeHandler / DoMeteorStrikeDamageHandler / CalcMeteorStrikeEffectHandler, GrandMagicResultHandler, _GrandMagicLaunchMsgSend, __bSetConstructionKit, iGetMapLocationForbiddenSide, _bCrusadeLog, the /crusade and /endcrusade GM commands, CNpc::behavior_manaCollector / _detector / _grandMagicGenerator and the mana stone; the crusade branches elsewhere (towers in npc_ai.rs, hammer / GMG / same-side rules and the fighter bonus in combat.rs and magic.rs, rewards in npc_ai.rs and life.rs, entry, recall, chat, shop prices, quests, guilds, mob generator) |
| `heldenian.rs` | the HeldenianMap / HeldenianTower / HeldenianModeMap / HeldenianWinningZone / HeldenianGateDoor map info lines, _CreateHeldenianGUID / bReadHeldenianGUIDFile, StartHeldenianMode, UpdateHeldenianStatus, SetHeldenianFlag, HeldenianEndWarNow, HeldenianPlayerKill, CheckHeldenianResultCalculation, RequestHeldenianScroll, GetOccupyFlagHandler, DeleteOccupyFlags + CMap::iRegisterOccupyFlag, CClient::CheckNearbyFlags / IsHeldWinner / IsHeldLoser, the summon scrolls and flag items of UseItemHandler, /heldenian and /endheldenian, the GSM_STARTHELDENIAN / GSM_ENDHELDENIAN relay; its branches elsewhere (mob generator, summons, deaths, drops, shop prices, flags bonus, tower kills, entry) |
| `apocalypse.rs` | the ApocalypseMap / ApocalypseMobGenType / ApocalypseBossMob / DynamicGateType / DynamicGateCoord map info lines, Global/LocalStart/EndApocalypse(Mode) + their GSMs, Notify_ApocalypseGateState, Use_ApocalypseGate, GenerateApocalypseBoss, GenerateSlime, SendThunder, DoAbaddonThunderDamageHandler, the DeleteNpc part (an emptied map), DELAYEVENTTYPE_KILL_ABADDON / END_APOCALYPSE, /apocalypse and /endapocalypse, the apocalypse-map recall of the 3 s timer and the x1.2 attack on those maps |
| `astoria.rs` | Astoria.cpp (CAstoria: capture the relic / destroy the shield, relic pick-up and drop, victory, casualties), EventStart / EventEnd, NotifyEventStats / RelicInAltar / RelicGrabbed, ShuffleAstoriaBasePos, CreateAstoriaFlags, DeleteFlags, UpdateRelicPos, m_eventWinner (../configs/stats.ini), /astoria and /endastoria; its branches elsewhere (base teleports, Teleport.cfg's astoria / middleland rules, the weekend recall, the relic holder's limits, deaths, slates, construction kits) |
| `scheduler.rs` | bReadSchedulerConfigFile (../configs/Schedule.cfg) and Scheduler (4 s timer: CTR / DS astoria events and the crusade at their minute, with the 2 h / 30 min / 10 min notices) |
| `gm.rs` | the '/' branch of ChatMsgHandler: /who, /send, /goto, /clearmap, /afk, /checkrep, /fi, /to, /setpf, /pf, /shutup, /rep+, /rep-, /hold, /tgt, /free, /closeconn, the /event* fight zone switches (AdminOrder_Event*) and _FightzoneEntry, /ban, /attack, /createfish, /teleport, /tp, /summonplayer, /unsummonall, /unsummonboss, /checkip, /polymorph, /noaggro, /invincible, /setinvi, /invi, /sethp, /setmp, /setmag, /gns, /setattackmode, /setforcerecalltime, /summon, /disconnectall, /createitem, /ci, the party commands, /shutdownthisserverrightnow, /setobservermode, /obs, the vanilla 3.82 ones (/kill, /revive, /summondemon, /summondeath, /unsummondemon, /clearnpc, /setzerk, /setfreeze, /safe, /peace, /unsafe, /war, /weather, /time: see "Vanilla 3.82 GM commands"); the GM chat log, m_onlineCntAdd; m_bIsAttackEnabled / m_magicLimited / the armour and shield bans / the permanent illusion of the maps |
| `notice.rs` | bReadNotifyMsgListFile (notice.txt) + NoticeHandler (3 s timer, every NOTICETIME), RequestNoticementHandler (the Login's noticement.txt) |
| `fightzone.rs` | FightzoneReserveHandler (MSGID_REQUEST_FIGHTZONE_RESERVE), FightzoneReserveProcessor (2-hour timer), GetFightzoneTicketHandler (COMMONTYPE_REQ_GETOCCUPYFIGHTZONETICKET), AdminOrder_ReserveFightzone (/reservefightzone), AdminOrder_GetFightzoneTicket (/getticket), m_iFightzoneNoForceRecall in the fight zone entry (the ticket use and the expiry at load were already in item_use.rs / player_data.rs) |
| `energy.rs` | EnergySphereProcessor (the /energysphere path), bCheckEnergySphereDestination (after the melee knock-back), the energy-sphere-creation-point / -goal-point map info lines |
| `gizon.rs` | StateChangeHandler (MSGID_STATECHANGEPOINT: the respec of 3 stat points for one majestic point, level cap characters only) + ChangeState + CheckMagicInt, GetDKItemHandler (MSGID_REQUEST_DKITEM: Gail's Dark Knight / Dark Mage armour and weapons) |
| `legion.rs` | the legion point services: the MSGID_REQUEST_LGNPTS / LGNSVC relays to the Login (bSendMsgToLS), MSGID_RESPONSE_LGNPTS (NOTIFY_LGNPTS) and MSGID_RESPONSE_LGNSVC: HandleLegionService (town change, gold, the legion hats and helms, majestic and reputation points; city hall only), ChangeNation, GetLegionGold, GetLegionItem, SubstractCash (MSGID_SUBCASH, after the save) |
| `reward.rs` | GetRewardMoneyHandler (COMMONTYPE_REQ_GETREWARDMONEY: the reward gold as a Gold item up to half the free load + NOTIFY_REWARDGOLD), GetHeroMantleHandler (COMMONTYPE_REQ_GETHEROMANTLE: the hero cape / helm / cap / plate / robe / hauberk / leggings for 100+ EK, HeroItemID / heroPrice) |
| `website.rs` | UpdateWebsiteStats / OnWebSocketEvent (1-minute timer): the HTTP POST of the online count to the GServer.cfg website, in a short-lived thread (std::net, no new crate) |
| `notify.rs` | SendNotifyMsg: the types layers 1-3 send (+ NOTIFY_REWARDGOLD) |
| `rng.rs` | MSVC rand()/srand(), Math.h ndice/dice |
| `consts.rs` | generated from the C++ headers (#defines only; enums are written by hand where used) |

## Run

    hg --dir <server folder> [--ws-port N] [--bind ADDR] [--login HOST:PORT] [--local-ip IP] [--seed N]

The folder is a copy of `run/server/Maps/All` (plus `../configs`). TCP clients on game-server-port,
WebSocket with --ws-port (path ignored). Test client: `cargo run -p hg --example hgprobe -- play LOGIN MAPDATA out.jsonl [--ws2 HOST:PORT]`,
`hgprobe clean` removes the rt* rows; scenarios: `fight`, `pvp`, `magic`, `items` (below). HG_TRACE=1 logs every socket event.

## Not ported on purpose (dead code in the C++)

Each of these is code the C++ never reaches (or reaches with nothing to do), so there is nothing to port; the reason is given for each.

- Taming (SKILLEFFECTTYPE_TAMING / _TamingHandler): no skill in Skill.cfg has effect type 3, so nothing calls it.
- The item logs of the TAIWANLOG build (buy, deposit, make, skill / magic learn, guild events, the civil right
  `ITEMLOG_APPLY`...): the macro is not defined in the deployed build (the non-TAIWANLOG `_bItemLog` lines are ported).
- Angels as companions (GetAngelHandler / MSGID_REQUEST_ANGEL): the angel item shop is commented out in the C++, no client
  can reach the handler legitimately (the pendants themselves are ported).
- Magic types 30 and 33: no spell in Magic.cfg uses them (polymorph, type 20, is ported although no shipped spell has it).
- The random energy sphere of EnergySphereProcessor(FALSE): its timer call is commented out in the C++ (only
  /energysphere makes one, ported).
- /mcount (Apocalypse_MonsterCount): NOTIFY_MONSTERCOUNT has no case in the C++ SendNotifyMsg, nothing is sent.
- GuildNotifyHandler (MSGID_GUILDNOTIFY): neither Login (C++ nor Rust) ever sends the message (no reference in either
  source), and the handler only copies the character and guild names into locals and returns: nothing to do even if it came.
- /dissmiss: the C++ handler is empty.
- GetMagicAbilityHandler (COMMONTYPE_GETMAGICABILITY): the C++ handler is empty. COMMONTYPE_REQ_TRAINSKILL: the call is commented out.
  CreateNewPartyHandler: its dispatcher case is commented out (parties go through MSGID_PARTYOPERATION, ported). CalculateGuildEffect
  starts with `return;`. RollMultiple (the second drop of a mob with several secondary drops) needs DropManager's tables, which the C++
  never loads (below).
- RequestCheckAccountPasswordHandler (MSGID_REQUEST_CHECKACCOUNTPASSWORD): neither Login (C++ nor Rust) ever sends it.
- bSendMsgToLS(MSGID_REQUEST_SETACCOUNTINITSTATUS) has no caller; MSGID_ITEMLOG is the TAIWANLOG build's (above).
- NpcDeadItemGeneratorWithItemEvent / bCheckInItemEventList: dead data. Only middleland's map info has `item-event` lines
  (8 September to 15 September, `Index item amount total month day type mobs`), the three item names in them are Korean
  strings that no Item*.cfg has (so `_bInitItemAttr` fails and nothing drops); they act only on those dates, so they could not
  be tested either.
- The crusade occupy flags: `__bSetOccupyFlag` starts with `return FALSE;` (so `_bDecodeOccupyFlagSaveFileContents`, its only
  caller, which nothing calls either, places nothing), `SaveOccupyFlagData` starts with `return;` (its callers' sends to the
  Login are commented out) and `DeleteOccupyFlags` is only called for heldenian maps (those flags are ported, heldenian.rs).
- The fight-zone part of KilledHandler: m_dwFightzoneDeadTime is written on a death in a `fight*` map; the only reader, a
  ResurrectPlayer after 30 s in the 1 s timer, is commented out. The _bPKLog lines are not written (log only).
- The spike field (dynamic object) does no damage in the C++ either.
- _Manager_Shutdown starts with `return;`; _Manager_Init only registers an external monitoring program that sends
  "?BWordMtr?" (no such program exists here).
- The other sectors' activity (neutral / aresden / elvine / istria / monster) of UpdateMapSectorInfo and the aged
  m_stSectorInfo: written, never read anywhere (only the busiest player sector, m_iMaxPx / m_iMaxPy, is read: ported).

## Kept as in the C++ (not bugs of the port)

- Guild notifications (NEWGUILDSMAN, DISMISSGUILDSMAN, GUILDDISBANDED) reach only the members on the same server, and members of
  a disbanded guild who were offline or elsewhere keep its name (the Login sends no guild-exists flag; the C++ read an
  uninitialised cGuildStatus). NOTIFY_SUCCESSBANGUILDMAN and COMMONTYPE_BANGUILD have no case in the C++ SendNotifyMsg: nothing is sent.
- Party chat ('$') is local to each server.
- A non-GM who enters a fight zone from the Login (INITPLAYER, not a teleport) is recalled to his town, unless one of his last 3 logouts
  from a fight zone was less than 18 s ago (m_recentDCs; the 4th logout within 10 minutes puts him in town). GMs are exempt. A player
  put there by SQL, or reconnecting after a long time, is thrown out at once.
- The special mob event (below) fires at the first 3 s tick after the start (m_dwSpecialEventTime starts at 0) and then every 5
  minutes, on the first map whose random generator rolls 3 in 1d10; it needs room on that map (the generator stops at
  maximum-object - 30 live objects), so on a full map the event waits. Its followers are created even when the master could not
  be (then without following anyone), and each starts where the last one ended up.

## Still not ported

Nothing live. The sweep of every `case MSGID_` / `COMMONTYPE_` / `GSM_` / `/command` of the C++ HG against the Rust one found three pieces after
batch C, all ported in batch E (reward.rs, website.rs; the `not ported yet` arms of the Rust dispatchers are gone for them):

- GetRewardMoneyHandler (COMMONTYPE_REQ_GETREWARDMONEY, HG.cpp 8098): hands the player's m_iRewardGold (the reward gold of
  enemy kills and PK captures, saved with the character) over as a Gold item up to half the free load, then NOTIFY_REWARDGOLD.
- GetHeroMantleHandler (COMMONTYPE_REQ_GETHEROMANTLE, HG.cpp 8050): the hero cape / armour for the player's side and sex
  (HeroItemID, heroPrice: EK + contribution), needs 100 EKs and a non-neutral player.
- UpdateWebsiteStats / OnWebSocketEvent (1-minute timer, HG.cpp 2440): POSTs the online count (clients + m_onlineCntAdd) to the website of
  GServer.cfg when its address, script and port are all set. A thread per request, 5 s connect / write timeouts, failures logged
  ("(!) Website stats: POST to ... failed"), the answer is not read (as in the C++). NOTE: the test GServer.cfg (hb-hgtest/All) names
  helbreathint.net/onlinestatus.php port 80, so the website lines must be commented out before a Rust HG runs there (the probes would POST
  to a real host); the unit tests and one manual run used a listener on 127.0.0.1 only.

MSGID_REQUEST_ANGEL and MSGID_MANAGER_INIT / SHUTDOWN are dead (above).

## Gameplay-visible fixes (the owner decides)

- CLIENTTIMEOUT_MS (silent drop of a client that sends nothing) raised from 18 s to 60 s (2026-11-22): the web client in a background tab sends its 8 s CHECKCONNECTION about once a second at best and was dropped while idle. The drop now logs `Client(name) timed out`. The same constant bounds the speed-hack time gap in `check_connection`.
- targetSearch: monsters hunt travellers (kept, see f2368c2).
- EnemyKillRewardHandler: `m_iEnemyKillCount + m_iEnemyKillAdjust;` had no assignment, so the C++ never raised the
  EK count; here an enemy kill at victim exp level >= 30 (80 for killers of level 80+) adds enemy-kill-adjust EKs. Kept: the owner decided (2026-11-20) that enemy kills add EKs.

GM commands (2026-11-21, thirteenth session): the owner decided (2026-11-21) to keep it.

- /clearmap re-entered (RequestTeleportHandler on the same spot) every player of the server when the GM stood on the
  cleaned map, and nobody when he did not: players elsewhere were bounced for nothing, and those of the cleaned map
  kept seeing the removed items. Here only the players of the cleaned map re-enter.

Social / events leftovers (2026-11-21): the owner decided (2026-11-21) to keep all the Rust fixes; the "Kept as in the C++" points stay as they are.

- None. Everything of this batch was ported as the C++ behaves (the special mob event, the friends list, the civil right, Kennedy,
  the fight zone recall of non-GMs entering from the Login); the only deviations are silent bounds / overflow fixes (below).

Batch D leftovers (2026-11-21): the owner decided (2026-11-21) to keep all the Rust fixes; the "Kept as in the C++" points stay as they are.

- StateChangeHandler never answered a refused respec: SendNotifyMsg has no case for NOTIFY_STATECHANGE_FAILED (nor _SUCCESS), so
  nothing reached the client, which had already moved its stat points locally and showed stats the server never set (the stat sum
  check and an empty majestic pool did not even return anything). Here every refusal after the init sends the 6-byte
  NOTIFY_STATECHANGE_FAILED, which the client handles ("Your stat has not been changed.", the local points reset).
- CheckMagicInt forgot the spells the lowered intelligence can no longer cast without telling the client, whose spell list showed
  them until the next login. Here NOTIFY_SPELL_SKILL (the silent reload of the spell and skill lists) follows the respec when a
  spell was forgotten.
- HandleLegionService's 10 / 100 reputation points were not announced (the client showed the old number until an exp message);
  here NOTIFY_EXP follows.
- GetDKItemHandler asks for nothing and checks nothing: any player, anywhere, can ask for any Dark Knight / Dark Mage piece as
  often as he likes and gets it free (the majestic cost is commented out in the C++; the client only offers the menu at the level
  cap, level >= MAXLEVEL). Kept as in the C++ (the intended rule is unknown); a forged message from a low-level player is the only
  way to notice it.
- The legion services take the cash after the service, and the Login checks it before: several requests sent before the first one
  is paid all pass the check, so a client that forges them gets several services for the cost of one (the Rust login then refuses the
  unpaid SUBCASHes, the C++ Login's query fails on the unsigned column). Kept as in the C++ and in both Logins.
- ChangeNation (the town change) leaves the player in his guild (a member of an Aresden guild stays in it as an Elvine citizen); only the
  location, the side flag and the party change. Kept as in the C++.

Magic / items leftovers (2026-11-21): the owner decided (2026-11-21) to keep all the Rust fixes; the "Kept as in the C++" points stay as they are.

- Polymorph (magic type 20): the spell set the target's type to 18 and its release (MAGICRELEASE ->
  Unit::RemoveMagicEffect) never gave m_sOriginalType back, because the spell has no status flag to hang a restore on:
  the target stayed type 18 (to everybody around) until it re-entered. Here the release restores the original type
  and shows it (OBJECTNULLACTION), for players and NPCs. (No shipped spell has type 20; the ones a server adds are
  the only ones affected.)
- CMap::bRemoveCropsTotalSum only counted down while the total was below MAXAGRICULTURE (200), so a map that reached
  200 planted crops could never plant again until a restart (bAddCropsTotalSum stops at 200 too, so the count stays
  there). Here it counts down from any value.
- NpcDeadItemGenerator compared the id of the tile's top item with ITEMTYPE_RELIC (13, which is the MainGauche+1
  weapon): a tile with one on top swallowed a monster's drop, while a drop on the real relic (id 1005) was refused by
  bSetItem but still announced (COMMONTYPE_ITEMDROP, the log, the expiry) for an item that was never put down. Here
  the drop vanishes quietly when the relic is on top, and nothing else swallows it.

Deaths / combat leftovers (2026-11-21): the owner decided (2026-11-21) to keep all the Rust fixes; the "Kept as in the C++" points stay as they are.

- ApplyElo gave the killer `m_elo = lvlFactor * K * (1 - E)` (`=`, not `+=`: his rating became the gain of that kill,
  8 at most) while the victim got `+=`. Here the killer's rating goes up by the gain too (`+=`), and the victim's E
  uses the new rating, as in the C++ order. Nothing shows or uses m_elo (it is only saved, as a WORD), so no player
  sees it; with both at 0 the first kill gives the same numbers (killer 3, victim -3 saved as 65533 for levels 60 / 40).

Batch E leftovers (2026-11-21): the owner decided (2026-11-21) to keep all the Rust fixes; the "Kept as in the C++" points stay as they are.

- GetHeroMantleHandler compared the contribution (a signed int) with the price (a uint32): a negative contribution (the energy sphere
  takes 10 from a scorer on the other side's goal) became a huge unsigned number, so the check passed and the price drove it further
  below zero: a player in debt of contribution got every hero piece for his kills alone. Here the compare is signed (a negative
  contribution cannot buy; the cape, which costs none, still can).
- GetRewardMoneyHandler with no reward gold left went on and handed over a Gold item of count 0 (an empty slot, or +0 on the stack) with
  NOTIFY_ITEMOBTAINED and NOTIFY_REWARDGOLD. Here nothing happens (the client's own button is for a positive reward; only a forged or
  repeated request reached it).
- Kept as in the C++: the reward gold is paid at one gold per weight unit of half the free load (a heavy bag gets it in several requests),
  and the hero items are bound to their owner (ITET_UNIQUE_OWNER).

Scheduler (2026-11-21, twelfth session): the owner decided (2026-11-21) to keep both.

- Any 4 s tick that announced nothing reset the entry to ES_ENDED, including the next tick of the same minute, so
  each notice (2 hours, 30 and 10 minutes before) went out about 7 times in its minute (8 seen in the test). Here
  once.
- The notice times added 2 hours / 30 / 10 minutes to the clock without carrying past midnight (hour 24, 25) or into
  the next day: an event at 0:00-1:59 got no 2-hour notice, and the 30 / 10 minute ones failed across midnight. Here
  they are counted on the week.

Astoria layer (2026-11-21, eleventh session): the owner decided (2026-11-21) to keep it.

- KilledHandler tested `(itemInd = HasItem(ITEM_RELIC))`: ITEM_NONE (-1) counted as found (a drop of bag slot -1)
  and a relic in slot 0 as missing, so a holder carrying it in his first slot kept the relic when killed. Here
  the relic always falls on the holder's death.

Apocalypse layer (2026-11-21, tenth session): the owner decided (2026-11-21) to keep all five.

- LocalEndApocalypse "restored" m_iMaximumObject with itself: the apocalypse maps (inferniaA/B, procella,
  abaddon) stayed at 0 objects and never filled up again until a restart. Here they get the map info's value back.
- An emptied boss map (abaddon) became gen type 1 for good, so the next apocalypse had no boss there until a
  restart; here the map info's gen type comes back at the end.
- LocalEndApocalypse, the DeleteNpc part and GenerateApocalypseBoss notified the gate states in a loop that
  returned from the whole function at the first empty client slot: at the end only the maps before that point got
  their gates reset (4 -> 3) and the "Apocalypse Mode OFF" step was cut short; an emptied map or a boss told only
  the clients before the first gap. Here everyone is told and every map is reset.
- GenerateApocalypseBoss sent the boss's x as its y (NOTIFY_MONSTEREVENT_POSITION on the minimap).
- DoAbaddonThunderDamageHandler returned at the first GM on abaddon: the players after him took no thunder damage.

Heldenian layer (2026-11-21, ninth session): the owner decided (2026-11-21) to keep all eleven.

- StartHeldenianMode / HeldenianEndWarNow only ran on the GM's server (GSM_STARTHELDENIAN / GSM_ENDHELDENIAN are
  defined but never sent): a heldenian started on the Events server left the towns' Gail charging for BtField and
  hiding it from all but the last winner, and one started in town had no towers. Here the start (type, GUID) and
  the end (type, side, both winners) go to every server.
- HeldenianEndWarNow zeroed the tower counts before comparing them, so the type 1 winner never changed, and
  returned when the server has no BtField, leaving the heldenian on (and the spawns stopped, below) until a restart.
- The towers HeldenianEndWarNow kills still counted down (the mode is on until its end): the counts stayed at -N,
  and the next type 1 heldenian started with 0 towers left, so destroying towers could never end it. Here the
  counts start from 0 again.
- MobGenerator returned in a type 1 heldenian: no monster spawned on any map of that server (with the relay, of
  any server). Here only the heldenian maps stop.
- _CreateHeldenianGUID wrote an empty file (its lines are commented out): the type never alternated (always 1,
  HRampart and GodH unused), the winners were lost at each restart, and IsHeldWinner (+1500 drop chance) /
  IsHeldLoser (x1.5 shop prices) never applied. Here the file keeps the GUID, both winners and the last type, so
  the winners' drops and the losers' prices apply after a heldenian.
- m_dwHeldenianGUID was never set, so a summon scroll bought in any heldenian stayed valid; each heldenian gets a
  GUID here and older scrolls are used up without effect (the C++'s own comment).
- RequestHeldenianScroll took the points only when there were enough, and gave the scroll anyway: free scrolls.
  Here no scroll without the points.
- GetOccupyFlagHandler with exactly 2 EKs gave a flag with an unset EK number and kept the EKs (endless flags);
  here a flag costs 2 EKs.
- SetHeldenianFlag returned TRUE when it placed nothing (outside a heldenian map, not a guild master...), so the
  flag item was used up for nothing; here it is kept (NOTFLAGSPOT).
- StartHeldenianMode set GodH's defenders' flag before turning the mode on, so the flag's own check failed and
  GodH never had it; here it is placed.
- RequestChargedTeleportHandler skipped the list's BtField rule (outside a heldenian, only the last type 1 winner):
  a forged index teleported anyone there. Here the same rules apply.

Crusade layer (2026-11-21, eighth session): the owner decided (2026-11-21) to keep all nine.

- NpcKilledHandler: in a crusade the exp of a kill was cut to 1/4 unless the killer was in the neutral map or a
  farm, but the test is `!= default || != arefarm || != elvfarm`, always true: the C++ cuts it everywhere. Here the
  neutral map and the farms keep the full exp, as the code means.
- MobGenerator: in a crusade the C++ `break`s out of its map loop at middleland, which stops the random generator
  and the spot generators of middleland and of every map loaded after it. Here only middleland's random
  generator stops.
- GSM_COLLECTEDMANA: the receiver read the two mana counts and dropped them, so the mana the collectors take from
  middleland's stones only reached the grand magic generators when the towns are on the same server; with the
  usual split (Towns / Middleland) no meteor strike could ever happen. Here the receiving server adds them.
- _CreateCrusadeGUID wrote an empty CrusadeGUID.txt (its two lines are commented out), so after a restart the
  GUID and the winner side were 0: the players of the last crusade who had not logged in since were not paid,
  and the winner's -10 % shop prices / the loser commanders' 3000 points were lost. Here the file holds both.
- RequestSummonWarUnitHandler counted every guild's war units (LWB, GHK, catapults...) in slot 0 (its loop
  returned at i = 0) while DeleteNpc took them off each guild's own slot: after 100 summons on the server nobody
  could summon until a restart. Here each guild counts its own (a guild with no location set yet cannot summon,
  CANNOTCONSTRUCT 3, as the C++'s own fallback says).
- The same handler kept the guild's count when the unit was not created (MS-Elvine, DT-Elvine, CP-Elvine, TK and
  BG are not in Npc.cfg), so each try used up one of the guild's 40 structures / 100 units. Here it is undone.
- RequestSetGuildTeleportLoc/ConstructLocHandler took the location from anyone (the client offers it to the
  commander only): a forged message from any guild member, or a guildless player, moved the guild's teleport and
  construction points. Here only the guild master in commander duty sets them.
- DoMeteorStrikeDamageHandler: a player with a magic protection 2 shield hit for 1..4 got -2..0 damage, i.e. was
  healed. Here the damage is at least 0.
- (port fix, not a C++ change) EnemyKillRewardHandler: the Rust port gave the reward gold (1d victim level) only
  at the EK level; the C++ gives it on every enemy kill, as now.

Gate-server / guild / party layer (2026-11-20, sixth session): the owner decided (2026-11-20) to keep all eight.

- Rust login: guild creation and disbanding checked the account in a "world name" read at 60..90 of the message,
  where the HG sends the guild GUID (creation, 75 bytes) or nothing (disband, 56 bytes): no account ever matched, so
  with either Login no guild could ever be created or disbanded. The Rust login now checks account + password only.
  (The C++ Login still rejects both: guilds work only with the Rust login.)
- '^' (guild chat over the gate server): the C++ relayed the line to the other servers even when it did not show it
  here (level 10 or less, dead); it is relayed only when it is shown.
- A party member entering a server (server change, re-login while the Login still lists it) had an uninitialised
  party status, so it shared no exp and could not leave with /dismissparty; it is CONFIRM again.
- A party join answered on another server is now shown to the members there (NOTIFY_PARTY 4), and a leave (op 6) is
  matched by name, so the members on the other servers are told (the C++ matched our client handle there).
- The party HP/MP coordinates sent the receiving member's max HP as the player's; the player's own now.
- A server change always marks the member as changing servers (op 7; the Login drops it after 20 s if it never
  arrives) and an unsaved logout always leaves the party (op 4); the C++ did both only for a killed player.
- Guilds: a ticket request is remembered, and a master's approve/reject must answer one (the C++ took any approve
  from anyone: a forged message put any same-side player into the sender's guild, even "NONE", or threw anyone out);
  the master cannot /ban himself; empty and "NONE" guild names are refused.
- The master's guild GUID is the Login's GuildID from the creation answer (the C++ kept a time-based GUID until the
  next login, so '^' chat between the master and the members who joined meanwhile broke at each relog).
- GetExp party share: the C++ gave every living local member exp x members + 0.75 (`dV2 = dV1 * iPartyTotalMember`);
  here each gets exp / members + 0.75 (as most Helbreath sources), plus 1/10 of that to the killer for its own kill
  as before. The owner decided (2026-11-20) to divide. Members anywhere on the server share (no distance check), as
  in the C++.

Item layer (2026-11-20, third session): the owner decided (2026-11-20) to keep all three.

- The bank now keeps its items. The C++ save never sent the bank part (see the save-block fix below), so with the
  deployed C++ server an item given to the warehouse keeper vanished at the next logout (seen in the items probe:
  C++ HGserver, bank row "" after the logout, the dagger gone). The Rust HG sends it when the bank changed; both the
  Rust login and the C++ Login store it (no layout extension: it is the C++ layout the C++ HG built and cut off).
- RequestRetrieveItemHandler sent NOTIFY_CANNOTCARRYMOREITEM for a bank item heavier than the free load and then
  handed it over anyway (no return); here the item stays in the bank.
- bEquipItemHandler, when it refused an item (stat limit of armour, banned shields/armour), released the item already
  worn in that slot without telling the client (which was told the *refused* item was released): the stats of the old
  armour were lost while it still showed worn. Here the old item stays on.

The five magic fixes below are kept: the owner decided (2026-11-20) to keep all of them.

- checkResistingIceSuccess compared the body slot's item *index* with the Santa costume ids 770/771, so the costume
  never protected from ice; here wearing it makes the wearer immune, as intended.
- MAGICTYPE_ICE (chill wind, ice strike...): a neutral player in the area made the C++ `goto MAGIC_NOEFFECT`, ending
  the whole area loop (tiles after him were not hit); here only his freeze is skipped.
- CMagic::Effect_RecSP ignored its arguments: the area tiles of staminar recovery restored the spot amount (4d8+8)
  instead of the area values (2d6+5); here the area values.
- ArmorLifeDecrement (armor break): m_wCurLifeSpan is a WORD, so armour with less endurance left than the decrement
  wrapped to ~65000 and never broke; here it breaks (endurance 0, ITEMLIFESPANEND, released).
- CNpc::magicHandler (NPC lightning bolt, type 19): the dead-tile and area hits used the linear *range* as the
  damage bonus; here the linear bonus.

Trade layer (2026-11-20, fourth session): the owner decided (2026-11-20) to keep it.

- ConfirmExchangeItem's weight check assigned each item's weight (`iItemWeightA = ...`) instead of adding it, so only
  the last item of each side counted: a trade could overload a player (the C++ accepts it; the bag then shows more
  weight than the player can carry). Here the sum must fit, so such trades are cancelled (CANCELEXCHANGEITEM).

Crafting layer (2026-11-20, fourth session): the owner decided (2026-11-20) to keep it.

- BuildItemHandler took material x's count from ware slot x whichever material that slot had matched (a recipe
  whose wares come in another order than its config used up the wrong amounts), and only required some of a slot's
  count left (`iItemCount > 0`), so one stack could pay for two materials with less than their sum (e.g. 3 of a ware
  for two materials of 2 each, the item built and the stack set to 0). Here each ware gives up the count of the
  material it matched and must hold it: such builds now fail (BUILDITEMFAIL) instead of succeeding cheaper.

- Water tiles: the C++ counts only tile sprite 19 (sea) as water, but 25 of Aresden's 29 and 23 of Elvine's 27 fish  points sit on the town lake tile (367), so fishing in the towns never worked. Here 367 is water too. Kept: the  owner decided (2026-11-20). (Item.cfg was also renumbered on 2026-11-20 so rods/manuals use the current skill  ids; with that, `hgprobe fishing` catches fish on arefarm and in Aresden.)
Skill / slate / upgrade layer (2026-11-20, fifth session): the owner decided (2026-11-20) to keep all six.

- CheckTotalSkillMasteryPoints only lowered a skill when a down skill was set (m_iDownSkillIndex, -1 by default):
  without one the total passed 900 freely. Here, with no down skill (or a down skill at 0), the lowest-SSN skill
  of 21+ (never the rising one) goes down, as the C++ already did for a down skill at 0: the cap holds.
- Pretend corpse: the C++ tested m_bSkillUsingStatus[19] (the old index; the array has 11 skills), which reads
  m_iSkillUsingTimeID's bytes (practically always 0). Here skill 10: while playing dead HP/MP/SP do not regenerate
  and the combat mode cannot be toggled, and a stop / attack / pick-up / cast first puts the player back on the
  tile (the C++ left them off it, unhittable, until they moved). Skill 10 is loaded and saved only by the Rust
  login + Rust HG (see "Skill 10 in the player data" under next steps); with a C++ side it is always 0.
- FishProcessor raised the catch chance with m_cSkillMastery[1] (agility, fishing's old index); here fishing (5).
- A failed Merien-stone cape upgrade kept the stone (free retries); here it is used, like every other failure.
- Wands upgraded with a Stone of Xelima were capped at +20, past the 4-bit level: +15 -> +16 wrapped to +0 (stone
  used). Here they stop at +15 (stone kept, like the other caps).
- A slate's expiry (DELAYEVENTTYPE_ANCIENT_TABLET) cleared the first slate bit set (red, then blue, then green),
  not its own: with two slates running the first expiry ended the other one. Here each event clears its own.

## Resurrection and angelic pendants (2026-11-20, fourth session)

Ported: MAGICTYPE_RESURRECTION (GetDeadOwner, same side unless GM, the caster's special ability used for 1200 s and
spell 94 removed: SPELL_SKILL + SPECIALABILITYSTATUS (1, 55, 1200)), NOTIFY_RESURRECTPLAYER, RequestResurrectPlayer
(yes: alive with max HP / 2, RequestTeleportHandler type 2 on the same spot; an unasked yes disconnects: the
"resurrection hack"); the pendants' stat bonus (CClient::SetAngelStr/Dex/Int/Mag + Get*: HP/SP or MP capped and
notified, SetAngel's status bits 0x1000-0x8000 with an OBJECTNULLACTION, cleared and set again by CalcTotalItemEffect,
removed on release before SETTING_SUCCESS).

Fixes: RequestResurrectPlayer resurrected a player who had restarted meanwhile (HP set to max / 2 and a teleport):
here a yes from a living player only clears the flag; SetAngel at load broadcast the not-yet-entered player.

    hgprobe resurrect LOGIN MAPDATA out.jsonl     (examples/hgprobe/magic.rs)
    hgprobe angel LOGIN out.jsonl                 (examples/hgprobe/items.rs)

resurrect: RtHg1 (aresden, level 140, int/mag 150) equips a ResurWand(MS.20) (SPELL_SKILL: the wand grants 94) 4 tiles
from RtHg2 (aresden, 3 HP), who casts Fire-Wall next to itself (again every 10 s: the 80 % roll) until it dies; RtHg1
casts 94 on its tile; RtHg2 answers yes, re-enters huntzone1; a second yes disconnects it; the saved HP is above 1.
angel: AngelicPendant(STR) (attribute 0x30000000: +4 str) equipped / released on gshop_1.
Results (2026-11-20, C++ Login): resurrect Rust 14/14, C++ 14/14 (first C++ run: the fire wall missed its roll, hence
the retry); same caster messages, byte-identical: SPELL_SKILL 117, SPECIALABILITYSTATUS 12 (1, 55, 1200), the magic
event 18, MP 10; RESURRECTPLAYER 6; after yes the teleport sequence to RESPONSE_INITDATA (sizes differ with the objects
in view). angel 7/7 in both: equip -> OBJECTNULLACTION 43 x2 (the second with status 0x1000) + SETTING_SUCCESS 34;
release -> NULLACTION, SETTING_SUCCESS 34, NULLACTION (bits cleared).

## C++ bugs fixed (a `// fix:` comment at each, except the two marked *)

- Batch D leftovers (2026-11-21, nineteenth session): GetDKItemHandler built the item name from an uninitialised buffer for an
  unknown item number or a sex that is neither MALE nor FEMALE (it logged "Gail asked to create a wrong item!" and went on);
  GetLegionItem did the same for the sex; the Login's handle in MSGID_RESPONSE_LGNPTS / LGNSVC indexed m_pClientList unchecked
  (a WORD: up to 65535); iRequestPanningMapDataRequest sent iSize + 17 bytes for an iSize + 11 byte message (6 bytes of stack
  garbage after the tiles); the iComposeMoveMapData / WriteTileData reads outside the map near the borders are the fix of the
  first layers. Port bug found by the legion probe: every item made with Item::new had count 0 (CItem's constructor sets 1), so
  the heldenian summon scrolls and flags and the astoria relic were made with count 0; Item::new now starts at 1.
  Kept as in the C++: NOTIFY_STATECHANGE_SUCCESS is never sent (the respec answers GIZONITEMUPGRADELEFT + SETTING_SUCCESS); the
  respec needs the six stats to add up to exactly 179 * 3 + 70 = 607 (only a level 180 character with every point spent), so
  after one respec the 3 freed points must be spent (MSGID_LEVELUPSETTINGS) before the next; a guild master cannot go below
  20 charisma; LGNSVC only works in the city halls (map name starts with `cityhall`).

- Social / events leftovers (2026-11-21, eighteenth session): RequestUpdateFriendsHandler strcat'ed the friend names into a
  256-byte buffer and sent them in another 256-byte one with no bound (a long list overflowed the stack; here the names that
  fit are asked, the message stays within 256 bytes); an empty friend token (a leading ", ") made the C++ loop spin on the same
  token for the 13 turns (here the loop ends, the same result; GSM_RequestFindFriend too); CMap::IncPlayerActivity indexed the 60 x 60
  sectors with the player's x / 20, y / 20 unchecked.

- Magic / items leftovers (2026-11-21, seventeenth session): RequestSetRecallPoint's range test `< 1 && > 5` was never
  true, so the signed byte of the message indexed m_pInitialPoint (20 entries) unchecked; a polymorph cast on an empty
  tile still registered a release event of owner type 0 (read through a stale `unit` pointer by the delay
  processor); __bSetAgricultureItem kept its naming value on the "structure on the tile" and "not farmland" refusals;
  the hoe's wear test `iTemp != -1 || item != NULL` read m_pItemList[-1] with no weapon in the right hand;
  bGetIsFarm's border test was missing (the Rust map only had the tile flag).
- Fight zones (2026-11-21, fifteenth session): /reservefightzone n indexed m_iFightZoneReserve[n - 1] unchecked.
  Kept as in the C++: every /reservefightzone toggles the forced recall of fight zones (with or without a zone);
  a reservation keeps the client's slot number until the 2-hour clear, even after he leaves; a ticket request
  with no tickets left negates m_iFightzoneNumber; the ticket count 50 is a byte.

- Noticement / energy sphere (2026-11-21, fourteenth session): EnergySphereProcessor with no naming value left
  created no sphere but still set a goal and announced it; the creation / goal points were indexed by the map
  info's number and by dice(1, total) unchecked (past the 10 points). m_iPrevSendNoticeMsg was never initialised
  (here -1). Kept as in the C++: notice.txt only counts `notify_msg = text` lines and the run/server copies have
  none (plain lines), so no notice is ever sent; NOTICEMSG goes to every client, entered or not; an empty goal
  point leaves the sphere with no goal (it cannot score) and blocks the next /energysphere on that map.

- GM commands (2026-11-21, thirteenth session): /send with no map name used an uninitialised name; /gns with no
  name read past the token. Kept as in the C++: /createitem's and /polymorph's argument quirks, the online count
  padding of /who (m_onlineCntAdd), the GM chat log sent to the Login.

- Astoria (2026-11-21, eleventh session): /endastoria with no event called m_astoria->GetVictoriousSide() on a
  null event (a crash); an event started on a server without astoria indexed the map list with -1; the base
  teleports read m_astoriaBasePos[digit] unchecked and Gail's astoria start point read past the bases for a side
  with none.

- Apocalypse (2026-11-21, tenth session): GenerateApocalypseBoss went on with m_pNpcList[MAXNPCS] when the boss was
  not created; DELAYEVENTTYPE_KILL_ABADDON killed whatever NPC held the slot 5 minutes later; the abaddon
  thunder tested m_bSkillUsingStatus[19]; GSM_ENDAPOCALYPSE is 5 bytes from both HGs but both Logins relayed and
  skipped 18 (13 bytes of the messages after it), fixed in the Rust login and read as 5 here.

- Heldenian (2026-11-21, ninth session): SetHeldenianFlag's server flag (code 2) logged through m_pClientList[-1];
  CMap::iRegisterOccupyFlag never marked the tile, so flags piled up on one tile (one per tile here). The Rust login
  relays GSM_GRANDMAGICRESULT at the full 19 + (count + 1) * 2 bytes the Rust HG now sends (crusade fix) and the
  two heldenian GSMs.

- Crusade (2026-11-21, eighth session): the guild-location replacement wrote m_pGuildTeleportLoc[MAXGUILDS] (past
  the array; the construction one also wrote the teleport map name); CreateCrusadeStructures marked iMSIndex[0..3]
  with fewer than 4 mana stones (index -1); Crusade.cfg numbers and map-info strike point numbers were written into
  the 300 / 20-entry arrays unchecked; a crashed strike point whose building map is on another server indexed the
  map list with -1; GSM_ENDCRUSADE was read as 17 of its 18 bytes and GSM_GRANDMAGICRESULT sent 1 byte short and
  read without its strike point list, so the stock messages after either were misread; a guildless player (GUID 0)
  matched a free guild slot for the guild teleport (an empty map name); __bSetConstructionKit kept the naming value
  on its early returns; the meteor damage tested m_bSkillUsingStatus[19] (pretend corpse is 10).

- Quests (2026-11-20, seventh session): QuestAcceptedHandler read m_pQuestConfigList[m_iAskedQuest] unchecked (an
  accept without a talk crashed the server) and never cleared m_iAskedQuest, so a client could accept again and
  again without talking: each accept restarted the quest past the search's level, side and contribution-limit
  checks (the normal client only accepts from the talk dialog, so no visible change). Now an accept takes the
  asked quest and clears it. NpcTalkHandler read the reward item's config unchecked, InitPlayerData the player's
  quest config (a quest removed from Quest.cfg crashed the entry), __iSearchForQuest m_cSkillMastery at the
  config's skill number.

- Gate server: ServerStockMsgHandler left the loop after the first whisperer of a GSM_DISCONNECT (goto) and fell
  through from GSM_REQUEST_SETFORCERECALLTIME into `default`, dropping the stock messages after either; crusade /
  apocalypse messages it does not port are skipped at the Login's sizes (the C++ misread their tails); a
  GSM_WHISPERMSG longer than 256 bytes overflowed a stack buffer; ChatMsgHandlerGSM stopped at the first client still
  logging in (`break`); NOTIFY_FRIENDONGAME copied 14 bytes from "" (zeros here).
- Guilds: RequestDisbandGuildHandler did not check the client; ResponseCreateNewGuildHandler wanted level and
  charisma 20 again, so a player who lost them meanwhile kept the guild name without an answer.
- Parties: the op-3 bounds check `(h < 0) && (h > MAXCLIENTS)` was never true; a player could ask itself to party.

- Map tiles indexed `x + y*sizeY` (wrong on non-square maps); WriteTileData read outside the map near borders.
- _bRegisterMap set m_iGodHMapIndex for every map (*: no map index globals are kept yet).
- mobevent-amount never left its read mode, so the rest of that map's info file was ignored and the read failed; spot-mob-generator with a type other than 1/2 read the next tokens as types.
- Config/map decoders wrote tables at unchecked indexes (atoi(token)).
- _bCheckSubLogSocketIndex ran past MAXSUBLOGSOCK; a failed sub-socket was reconnected to the client port (*: every reconnect goes to the gate address).
- CheckClientResponseTime `break`s out of the loop for everyone when one player was deleted.
- InitPlayerData tested an uninitialised cGuildStatus; the fight-zone reserve notify went to client `i` (a loop counter); the sprite type was uninitialised for an unknown sex.
- PLAYERITEMLISTCONTENTS was sent 2 bytes short; the bag compaction did not move m_sItemEquipmentStatus.
- ValidateSkills zeroed the SSN (progress) of every skill not in its list at each login.
- bEquipItemHandler: one-hand weapon speed was or-ed into the appearance temp, so the status lost it; at load it also broadcast the not-yet-entered player.
- Save block: quest ID written as a WORD; the bank (count + items) never sent even when modified.
- Teleport tile to another server did not set m_bIsOnServerChange, so the disconnect saved again and logged the account out mid-change; scripted teleports strcpy'd a NULL map name.
- NOTIFY_WHISPERMODEON/OFF copied 10 bytes from the 2-byte literal " "; move reject sent an uninitialised byte.
- bGetEmptyPosition passed uninitialised coordinates when the map has no initial point; iGetMapLocationSide read name[-1] for 1-char names; ChatMsgHandler wrote/read past short messages.
- Settings.cfg values missing from the file were uninitialised.

Layer 3 fixes:
- bSetNpcAttackMode found the guard bAnalyzeCriminalAction had just created by the first 5 bytes of its name
  (`_q184` also matches `_q1845`); here by handle.
- bCreateNewNpc divided by zero for a zero-width random area or a single random waypoint; _cGetSpecialAbility returned an
  uninitialised char for kinds outside 1..10.
- CalcTotalItemEffect multiplied the small/medium attack dice and bonus by 100 into a `char` (wrapping: a 1d8 sword
  became dice(100, 32) + garbage); it also sent the "special ability changed" notify to client `i`, an uninitialised
  loop variable.
- calculateAttackEffect added the target's defense shields (DS/GDS) only when the attacker held a two-handed non-bow
  weapon; they count against every non-bow attack here.
- iClientMotion_Attack_Handler read m_pNpcList[sOwner] whenever the tile owner's handle matched the target id - 10000,
  even for a player owner or a vanished NPC; the owner type is checked here.
- NpcBehavior_Flee dereferenced a target that had vanished.
- targetSearch: side 10 -> HOSTILE made the monster branch unreachable (monsters ignored travellers). Kept on purpose: the owner decided (2026-11-20) that monsters hunt travellers, even though the deployed C++ server lets them be.
- Random mob generator: tables ending in `iResult < 100` left a roll of 100 unmapped (levels 4, 8-10, 12-16), which
  spawned the default Orc; the follower count ran on uninitialised iResult/bMaster when no naming value was free;
  bFirmBerserk only set the status bit (SetMagicFlag), so the berserk damage bonus never applied; bSetNpcFollowMode
  matched names on 5 bytes (done by handle here); followers of a dead master followed whatever NPC reused its slot.
- TileCleaner sent SETITEM only when a tile became empty (a cleaned top item stayed drawn) and also for items already
  picked up; DeleteNpc registered pItem instead of the slate with AddGroundItem (slates never expired).
- InitStats took the jewel bonus as uint32 (a negative value wrapped dice()'s range).
- NOTIFY_ITEMOBTAINED sent 2 uninitialised stack bytes after the attribute (zeros here).

Magic layer fixes:
- CheckResistingMagicSuccess on an empty tile read uninitialised resistance/protection (and usually rolled a dice);
  empty tiles are skipped here (no dice for them, nor the wasted damage roll of Effect_Damage_Spot).
- MAGICTYPE_CREATE with a value other than 1 used an uninitialised item id; the NPC spell table of magic level 13
  dereferenced spell 96, which Magic.cfg lacks (NULL); CREATE_DYNAMIC fields checked bAnalyzeCriminalAction on the
  tile past each row; ShowClientMsg copied 8 bytes from the 7-byte "Server".
- The delay.rs port had MAGICTYPE_ICE_LINEAR = 28 (it is 27): a port bug, not the C++.

Item layer fixes:
- GiveItemHandler read m_pItemList[sItemIndex] before checking the index; a stack part given to oneself was deleted
  (amount lost) and a whole item moved slots: a no-op here. bSetItemToBankItem sent 56 bytes of a 54-byte message
  (2 uninitialised bytes: zeros). RequestRetrieveItemHandler indexed the bank with an unchecked signed byte.
- ReqSellItemHandler / ConfirmHandler left dwMul2 uninitialised for primary stats 13-15 (x1 here; no item has them).
- ReqRepairItemHandler: a negative difference (endurance above the maximum) became a huge DWORD price (0 here).
- RequestPurchaseItemHandler took the price as a WORD (an item over 65535 gold would cost price % 65536; none
  costs that much) and looped over the client's count after the bag was full (one CANNOTCARRYMOREITEM per item).
- _bDepleteDestTypeItemUseEffect used a dye up when the target slot was out of range or empty.

Trade layer fixes:
- SetExchangeItem tested `m_exchangeCount > 4`, so a fifth item was written past m_exchangeItems[4].
- ExchangeItemHandler accepted amount 0 and the trader's own tile (a window with oneself that never completes);
  ConfirmExchangeItem reset the confirmer's m_exchangeCount twice and never the partner's.
- A non-stackable item (not CONSUME/ARROW) with a count above the traded amount had its count set to the amount, the
  rest lost; here the item moves whole (normal items have count 1: no visible change).
- NOTIFY_GIVEITEMFIN_ERASEITEM (also GiveItemHandler's) copied 20 bytes from the 10-byte char name, sending the
  bytes after it: the receiver's account name (seen in the trade probe: "rthg1"). Zeros here.

Crafting layer fixes:
- BuildItemHandler accepted ware slot MAXITEMS (`> MAXITEMS`, one past the bag); crafting stamped the item's touch
  value 3 with "%d%2d" (day 5 of month 11 -> "11 5" -> 11): month*100+day here, like the shop and the drops.
- Kept as in the C++: NOTIFY_BUILDITEMSUCCESS tests `sV1 >= 0` on a DWORD (always true), so a negative quality goes
  out as a plain short, which the client shows correctly (its +10000 branch is never used); BuildItem's touch value 3 is
  the truncated timeGetTime(); a player who sends 2 or more potion/craft/build requests between two 3 s checks is
  disconnected (CheckClientResponseTime's m_iSkillMsgRecvCount anti-macro).

Batch E fixes (reward.rs, website.rs):
- GetHeroMantleHandler indexed HeroItemID[req][side - 1][sex - 1] with the hostile side (4: past the table) or a sex of 0 (-1: the
  neighbour's entry): refused here; a sex / side outside the table gets nothing.
- UpdateWebsiteStats replaced m_webSocket with a new XSocket every minute, leaking (and never answering) the previous one when its connect
  outlasted the minute; every POST is its own short-lived thread here, with a 5 s connect and write timeout.

Skill / slate / upgrade / item log fixes:
- MineralGenerator / FishGenerator: dice(1, 0) - 1 indexed point -1 on maps without points, and an unset point
  `break`s the map loop; FishGenerator went on with ITEM_NONE and an uninitialised difficulty; FishProcessor
  `break`s out for everyone on a vanished fish; a fish's expiry sent its DYNAMICOBJECT reject twice; DeleteClient
  left an engaged fish counted.
- DELAYEVENTTYPE_ANCIENT_TABLET dereferenced a missing client and sent an uninitialised slate type when no bit
  was set; ReqCreateSlateHandler read m_pItemList[k] before its off-by-one range check (signed byte index).
- _CheckMiningAction dereferenced m_pMineral[v1] unchecked; the DK / dark-mage / cape conversions left an
  uninitialised item in the slot when the new id was missing (the old item stays here).
- Deaths / combat leftovers (2026-11-21): the Fire-Bow tests `iGetAttribute(sX, sY, 0x06)` with the C++'s -1 within 20
  tiles of the map border (always "set"), kept; ApplyElo is the only fix there (see the pending list above).
- Port bug fixed: MAGICTYPE_CREATE (food) dropped an item of count 0.
- Kept as in the C++: iCheckFish marks skill 1 in use (the old fishing index; the FishingRod's related skill in
  the item config is 1 too); the plain cape's Cape+1 conversion takes 50 contribution without checking for them;
  the hero capes' +1 form keeps level 0; an upgrade roll at the level cap can still fail and break the item.

## Notes for later layers

- The C++ never calls DropManager::InitDrops (commented out), so `m_drops` is empty there: nothing drops from the XML tables
  (HasPrimaryDrop/HasSecondaryDrop are false). droplist.xml is loaded here (387 drops) but, like the C++, nothing uses it.
- dwGoldDropValue is never loaded (the npc.cfg column is commented out in _bDecodeNpcConfigFileContents), so NPCs never
  drop gold on death and the "stop after a hit" roll is dice(1, 3). Kept as in the C++.
- GetExp uses the float constants of the C++ (0.025f, 1.025f): level 41 gives x1.99999999, not x2.
- m_dwTime (lag protection: a player whose last message is older than RAGPROTECTIONTIME cannot be hit) is updated on
  every read, like XSOCKEVENT_READCOMPLETE.
- Object handles on the wire are client slots 1..199 (NPCs +10000), allocated lowest-free like the C++.
- SendEventToNearClient picks the frame key with rand(); with `--seed` the sequence is the MSVC one.

## Logout and the C++ Login (relogin scenario)

    hgprobe relogin LOGIN out.jsonl     (examples/hgprobe/items.rs)

DeleteClient sends SERVERSTOCKMSG [GSM_DISCONNECT][name 10][0] (18 bytes, the C++ m_iIndexGSS + 1) before the save.
The C++ Login's ServerStockMsgHandler sets IsPlaying = FALSE on it; its account timer then force-disconnects and
deletes the account entry 50 s (MAXWAITTIMEFORPLAYERENTERGAME, counted from the enter) + 12 s later. Without it (the
Rust HG before 2026-11-20) IsPlaying stayed TRUE and the entry was never freed: ENTERGAMERESTYPE_PLAYING until the
Login restarted. Results (2026-11-20, C++ Login): re-login accepted 60 s after the logout with both the Rust HG and
the C++ HGserver (logout 3 s after the enter), second session enters the game in both. The HG<->Login link cannot be
recorded with the bridge: the gate port 5656 is hard-coded in both HGs.

## Test: skills and mining scenarios (2026-11-20, fifth session)

    hgprobe skills LOGIN out.jsonl     (examples/hgprobe/skills.rs)
    hgprobe mining LOGIN out.jsonl     (only mineral point 0 (24,132) in the test copy's aresdend1.txt; restored)

skills: RtHg1 (aresden 110,56, agility 10 by SQL) uses a FishingManual, casts a FishingRod at (59,99), uses an exp
slate (Effect2 = 4), makes a slate from the four AcientTablet pieces, upgrades a Dagger with a StoneOfXelima and an
AngelicPendant(STR) with no Gizon points, sets down skill 1, tries skill 10, stops.
Results (C++ Login): Rust HG 14/14, C++ HGserver 14/14; byte-identical: SKILLTRAINSUCCESS 8 (1, 20) + ITEMDEPLETED 10;
SKILLUSINGEND 8 (0: the rod's related skill is 1, agility: GET with value 0, see next steps); exp slate own
OBJECTNULLACTION 43 (status 0x10010000) + ITEMDEPLETED + SLATE_EXP 6; slate creation 4 x ITEMDEPLETED 10 +
SLATE_CREATESUCCESS 10 + ITEMOBTAINED 53 (both succeeded; types 2 and 4: values only; the C++ 2 garbage tail
bytes); the dagger upgrade failed in both (ITEMATTRIBUTECHANGE 20 idx 7 attr 0, then the stone's ITEMDEPLETED);
ITEMUPGRADEFAIL 8 (3); DOWNSKILLINDEXSET 8 (1); skill 10: SKILLUSINGEND 8 (a new character has it at 0).
(The run/ Item.cfg was renumbered on 2026-11-20, FishingManual -> 5, PretendCorpseManual -> 10: since then the
probe's SKILLTRAINSUCCESS (1, 20) and rod checks fail in both Logins, 11/14, as data, not code.)

    hgprobe corpse LOGIN out.jsonl [lost]   (examples/hgprobe/skills.rs)

corpse: RtHg1 (aresden 110,56, level 50 so ValidateSkills keeps it) with pretend corpse 100 in the DB plays dead,
the probe sets the row to 7/0 behind the server, logs out, and checks the row came back 100/8 (the HG saved
mastery and the SSN +8); enters again, plays dead, row 100/16; skills 0..9 unchanged. Rust login + Rust HG 14/14.
`lost` (C++ Login + Rust HG): SKILLUSINGEND both rounds, row 10 stays 7/0, skills 0..9 intact: 14/14.
mining: RtHg1 (aresdend1 25,131, PickAxe, mining 100, str 100) waits for the mineral and hits it. Rust HG 7/7: the
mineral (DYNAMICOBJECT confirm 18, type 4) appeared at once, 23 hits, ITEMDROP 18 on the miner's tile per success,
the mineral removed (reject) when used up. C++ HGserver 4/6: no mineral appeared in 240 s and the attacks on the
empty tile got no OBJECTMOTION_ATTACK_CONFIRM (the Rust HG answers them): not explained yet (gap to investigate:
the C++ MineralGenerator runs every 10 s from OnTimer in the source; the deployed binary may differ).

## Test: trade scenario

    hgprobe trade LOGIN out.jsonl     (examples/hgprobe/items.rs)

RtHg1 (gshop_1 51,41: Dagger, 1000 gold) opens the window on RtHg2 (52,41: 500 gold, WoodShield, RedPotion) with the
dagger and adds 300 gold; RtHg2 adds 200 gold and the shield; RtHg1 confirms (nothing yet), RtHg2 confirms; then
RtHg2 opens a second window with the potion and RtHg1 cancels it; the saved bags are checked.

Results (2026-11-20, C++ Login): Rust HG 19/19, C++ HGserver 19/19. Same messages, sizes and order: OPENEXCHANGEWINDOW
57 / SETEXCHANGEITEM 57 to both (slot + 1000 in the owner's own copy), no message on the first confirm; on the second
the confirmer gets ITEMOBTAINED 53 x2 (RtHg1's items), SETITEMCOUNT 13 (its gold 800 -> 600), GIVEITEMFIN_ERASEITEM 32
(the shield), EXCHANGEITEMCOMPLETE 6, and the other side GIVEITEMFIN_ERASEITEM, SETITEMCOUNT (700), ITEMOBTAINED x2,
EXCHANGEITEMCOMPLETE; CANCELEXCHANGEITEM 6 to both. All 22 trade messages are byte-identical except the
GIVEITEMFIN_ERASEITEM name tail (the account-name leak above). Saved rows equal: RtHg1 Gold 900 + WoodShield, RtHg2
Dagger + Gold 600 + RedPotion.

## Test: craft scenario

    hgprobe craft LOGIN out.jsonl     (examples/hgprobe/items.rs; 3.2 s between requests: the anti-macro above)

RtHg1 (gshop_1, alchemy 100 and manufacturing 100 by SQL; the Login loads manufacturing capped at str*2 = 80) makes a
RedPotion (SlimeJelly + AntLeg + SnakeTongue: 95 %), asks a potion of one jelly (NOMATCHINGPOTION), crafts RubyWare
(Crystal + Ruby x2, 70 %) and builds a SuperCoal (Coal x3, 80 %); the saved bag is checked.

Results (2026-11-20, C++ Login): Rust HG 11/11; C++ HGserver 11/11 on the second run (the first rolled the 20 %
BUILDITEMFAIL). Same messages, sizes and order: potion SETITEMCOUNT 13 (jelly 2 -> 1), ITEMDEPLETED_ERASEITEM 10 x2,
POTIONSUCCESS 6, ITEMOBTAINED 53 (byte-identical RedPotion); NOMATCHINGPOTION 6; crafting ITEMDEPLETED x2,
CRAFTING_SUCCESS 6 (or CRAFTING_FAIL 6 with nothing used up: the wares are consumables), ITEMOBTAINED (purity 20+1d80
in spec value 2, attribute 1); build ITEMOBTAINED 53 (SuperCoal, purity skill/2 + 1d(skill/2+1)-1: 63 Rust, 40 C++),
BUILDITEMSUCCESS 10 (purity, type 12), SETITEMCOUNT 13 x2 + ITEMDEPLETED_ERASEITEM 10 (the coal). Differences: values only.

## Test: fight scenario

    hgprobe fight LOGIN MAPDATA out.jsonl     (examples/hgprobe/fight.rs)

RtHg1 (level 30, traveller, exp one short of level 31) on aresden next to slime spot 1: combat mode on, hunt slimes
(type 10 only), kill 3, exp -> level up (travellers stop at 50, NOTIFY_TRAVELERLIMITEDLEVEL), +3 str
(LEVELUPSETTINGS), logout; RtHg2 with 3 HP next to slimes until killed, restart -> `default`. The C++ Login keeps an
account "playing" for a while after a logout, hence the second character; restart the Login between runs.

Since 2026-11-20 (second session) session 1 also waits for a kill drop (COMMONTYPE_ITEMDROP), walks onto it and
picks it up (OBJECTGETITEM -> NOTIFY_ITEMOBTAINED 53 bytes + COMMONTYPE_SETITEM), and lists the NPC types in view.
Run it with `primary-drop-rate = 10000` in the test copy's Settings.cfg (%LOCALAPPDATA%\hb-hgtest\All,
the original is Settings.cfg.bak), otherwise a slime drops something ~9 % of the time.

Results 2 (2026-11-20, C++ Login): Rust HG all ok (LightAxe / Dagger dropped with stats, picked up); C++ HGserver
session 1 all ok (WoodShield attr 0x824300 = STRONG 2 + HPREC 3), session 2 3 FAIL (the slimes never attack the
traveller: the C++ bug below). Identical layouts: ITEMDROP / SETITEM 18 bytes, ITEMOBTAINED 53 bytes (C++ 2 garbage
tail bytes, zeros here), the NPC status nibble 7 = 4 for monsters in both (Rust sent 10 before this session: fixed).
Value ranges match (weapons get a coloured stat, armour colour 0, touch value 3 = month*100+day). Rabbits/cats: the
server spawns them (debug count on aresden in 25 s: 19 rabbit, 3 cat, 4 slime, 4 ant, 2 orc masters), but they spawn
outside the town avoid-rect (76,70)-(209,219), so the probe at (110,56) only sometimes has one in view (neither run did).

Results (2026-11-20): Rust HG with the C++ Login 21/21, Rust HG with the Rust login 20/21 (the one FAIL was the
probe's damage check when the first hit killed outright, fixed since), C++ HGserver session 1 all ok. Message layouts
and sizes match the C++ for every combat message seen (EVENT_MOTION attack 13 / damage 11 / dying 15 bytes, NOTIFY HP
14, EXP 14, LEVELUP 34, TRAVELERLIMITEDLEVEL 10, ITEMOBTAINED 53, SETTING_SUCCESS 34, KILLED 26, attack confirm 6);
bare-hand hits on slimes were 1..3 in both. Differences: the C++ drops an item on a kill (COMMONTYPE_ITEMDROP:
NpcDeadItemGenerator, gap) and shows rabbits/cats (random mob generator, gap). Slime question (settled): initNpcAttr turns npc.cfg side 10 into HOSTILE (4, Shared/common.h) but targetSearch
still tests `m_side < 10`, so every monster took the town-NPC branch, where a side-0 target with no PK is skipped:
C++ monsters never aggro travellers (RtHg2 is one) and only fight back when hit. C++ bug (the monster branch became dead
code); Rust keeps side 4 (status nibble 4 on the wire, like the C++) and treats HOSTILE as the monster branch, so
travellers are hunted as in the original Helbreath. Citizens (side 1/2) are attacked in both.

## Test: PvP scenario

    hgprobe pvp LOGIN MAPDATA out.jsonl     (examples/hgprobe/pvp.rs)

RtHg1 (aresden citizen, level 60, combat mode) on aresden kills, one victim at a time (the game server takes 2
clients per IP), RtHg2 (elvine, level 40, 5 HP: enemy kill), RtHg3 (aresden, PK 2: PK captured) and RtHg4 (aresden,
PK 0: same-town kill -> PK penalty + arejail). Restart the C++ Login between runs.

Each victim's death penalty drops items (penalty - 1 below level 80: RtHg2 1, RtHg3 2, RtHg4 none), and at the end
RtHg1 drops its first bag item with the client's drop command (COMMONTYPE_ITEMDROP).

Results 3 (2026-11-20, after bAnalyzeCriminalAction): Rust HG 37/37, C++ HGserver 37/37. Only the same-town kill
of an innocent calls a guard (none for the enemy kill or the criminal); the guard's EVENT_LOG appearance is 27 bytes
in both, on the same tile next to the killer (151,92), name `_<map><naming value>`, type 21. Remaining differences
are timing (regeneration notifies, how long the guard was watched moving) and the item list's 2 bytes (fixed earlier).

Results 2 (2026-11-20, after PenaltyItemDrop/DropItemHandler, fresh characters): Rust HG 34/34, C++ HGserver 34/34.
Identical layouts: DROPITEMFIN_ERASEITEM 12 (index, amount -1 on death / 1 for the command), EVENT_COMMON ITEMDROP
18 to the victim and the killer, same counts. The guard was the only message difference left (done since, Results 3); the C++
sends guild rank 65535 for a fresh character (-1) and 127 afterwards: the C++ Login stores the rank WORD (rank byte +
fight-zone byte = 0x00FF) in a TINYINT, so -1 comes back as 127 (the Rust login reads the byte).

Results (2026-11-20, C++ Login, first version): Rust HG all ok, C++ HGserver all ok except the save check (the C++
saves a few seconds after the logout; the probe now waits up to 20 s).
Same layouts, sizes and order: ENEMYKILLREWARD 48 + EXP 14; PKCAPTURED 28; PKPENALTY 38 then the jail teleport
(2965, 2898, 2906, 2908 206 bytes, 2978, 2983, 2968, RESPONSE_INITDATA on arejail); saved rows equal (RtHg1 arejail,
PK 1, lock arejail ~170 s; RtHg2 arejail lock 180; RtHg3 arefarm PK 1; RtHg4 arefarm, no exp loss). Differences: EK
0 in the C++ vs 10 here (the gameplay-visible fix above); the C++ victims drop items (NOTIFY_DROPITEMFIN_ERASEITEM
12 + COMMONTYPE_ITEMDROP 18 to the viewers: PenaltyItemDrop, done since) and a guard (type 21) appears next to the
killer after the PK (bAnalyzeCriminalAction, done since). The no-attack areas (_SetupNoAttackArea, tile attribute 0x04)
were not set up here before this session: fixed.

## Test: magic scenario

    hgprobe magic LOGIN MAPDATA out.jsonl     (examples/hgprobe/magic.rs; HG_MAGIC_DEBUG=1 logs the "(MAGIA)" trace)

RtHg1 (aresden, level 60, int 77 mag 90, MagicMastery set by SQL for 0 1 10 12 13 25 45) next to slime spot 1:
heal on itself, a cast without the precast (ignored), defense shield on itself (60 s), magic missile x2, energy bolt,
hold person, chill wind and energy bolt on slimes; RtHg2 (elvine) logs in with SwordofIceElemental (item row by SQL),
activates its SA (ice, 60 s) and hits RtHg1 until it freezes (30 s); RtHg1's recall is refused (hit < 10 s ago),
the SA runs out, the ice and the shield are released by their MAGICRELEASE delay events, then (after holding the
slimes next to it) RtHg1 recalls. Restart the C++ Login between runs.

Results (2026-11-20, C++ Login): Rust HG 27/27, C++ HGserver 27/27; pvp re-run on Rust 37/37 (after `hgprobe clean`:
the magic scenario replaces RtHg2's items with the SA sword, which breaks pvp's drop check).
Identical layouts, sizes and order: precast confirm RESPONSE_MOTION 6; heal NOTIFY_HP 14 -> COMMONTYPE_MAGIC event
18 (caster x, y, target x, y, spell + 100, caster type: same bytes) -> NOTIFY_MP 10; defense shield: own
OBJECTNULLACTION 43 -> MAGICEFFECTON 16 (11, 3); slime hits OBJECTDAMAGE 11 / OBJECTDYING 15 before the magic event;
SPECIALABILITYSTATUS 12 (1, 2, 60) and (3); MAGICEFFECTON/OFF (23, 1) identical bytes, released after 30-31 s;
shield released after 59-60 s; CANNOTRECALL 6 (mana still spent, like the C++); the recall's teleport sequence
(2965, 2898, 2906, 2908 206, 2978, 2983, then the magic event and MP on the new spot) in the same order.
Differences: only values (damage, which slime, how many level-ups) and the map-data sizes (objects in view). Neither
run saw a slime frozen by chill wind (resisted or killed). Slimes hit RtHg1 near the end in both (citizens are hunted),
hence the hold-before-recall step.

## Test: items scenario

    hgprobe items LOGIN MAPDATA out.jsonl     (examples/hgprobe/items.rs)

RtHg1 (gshop_1, 10 HP, Gold 5000, Dagger, WoodShield, 3 RedPotion, Meat by SQL): equip dagger, equip shield,
release dagger, drink a red potion, eat the meat, buy 2 red potions, sell the dagger (offer + confirm), ask a shield
repair at the shop. RtHg2 (wrhus_1): gives its Dagger to Howard (NPC type 20, found by REQUEST_FULLOBJECTDATA),
logs out (bank_item row checked), logs in again (bank list), retrieves it, logs out (bag row, bank empty). RtHg3
(wzdtwr_1, 1000 gold) buys Fire-Wall, then Energy-Strike (1000 gold: fails). RtHg4 (huntzone1, Fire-Wall set by
SQL: the tower is a no-attack area where attack spells fizzle) casts a fire wall on the tile east of itself and
stands there until the fire on that tile expires.

Results (2026-11-20): Rust HG + Rust login 40/40 (`restart_rl.sh` in the session scratchpad: Rust login with
`--ws-port 4002 --tcp-port 4000`, gs port 5656); Rust HG + C++ Login: all ok up to the bank, the bank row saved by
the C++ Login too, then the second login of RtHg2 was refused (ENTERGAMERESTYPE_PLAYING for 3+ minutes: the C++ Login
keeps the account playing after a Rust HG logout, but not after a C++ HGserver logout — see next steps);
C++ HGserver + C++ Login 37/41: the four bank FAILs are the C++ bug (the banked dagger is gone after the logout).
Identical layouts, sizes and order: equip/release own OBJECTNULLACTION 43 (appearance 2 weapon bits set / cleared,
same bytes); potion ITEMDEPLETED_ERASEITEM 10 then HP 14; meat ITEMDEPLETED_ERASEITEM 10 only; purchase
ITEMPURCHASED 48 then SETITEMCOUNT 13 (gold) per item, price 5 for a RedPotion (aresden citizen: 10 % off a 5 rounds
to 0), gold 4990; SELLITEMPRICE 42 (slot, life 300, price 12, count 1); ITEMSOLD 14, ITEMDEPLETED_ERASEITEM 10,
ITEMOBTAINED 53 (12 gold); CANNOTREPAIRITEM 30 (reason 2: a shield is not repaired at the shop); ITEMTOBANK 56 +
GIVEITEMFIN_ERASEITEM 32; RESPONSE_RETRIEVEITEM 8 (Rust: confirm 0, 1; C++: reject, the bank was empty);
MAGICSTUDYSUCCESS 37 (40, "Fire-Wall") after SETITEMCOUNT (760 gold), MAGICSTUDYFAIL 46 (1, 60, name, cost 1000,
int 67) same bytes; Fire-Wall: 5 MSGID_DYNAMICOBJECT confirm 18 bytes (x, y, type 1, id), COMMONTYPE_MAGIC event 18,
MP 10, then NOTIFY_HP every 3 s from the 3x3 fire damage (several fire tiles reach the caster: 2-3 HP drops per
tick in both), the fire on the target tile expired after 35 s (C++) / 36 s (Rust): 30 s + 1d4 s, checked every 3 s.
Differences: values only (potion roll, object ids: the C++ dynamic list already holds objects, id 38 vs 1), the
NPC name in the map data ("1x0" C++, "1x" Rust: 5-byte object name; not item layer, to check).
Regressions (2026-11-20, after the item layer, Rust HG): play 22/22, fight 23/23 (primary-drop-rate 10000), pvp 37/37 with the Rust login; magic 27/27 with the C++ Login, 26/27 with the Rust login (slimes kept hitting RtHg1, so every recall got CANNOTRECALL: the probe's environment, twice in a row; the item layer does not touch recall).

Regressions (2026-11-20, fourth session, after trade/crafting/resurrection/angels, Rust HG): play 22/22, fight 23/23,
pvp 37/37, magic 27/27 with the Rust login; items 41/41 with the C++ Login (the re-login after the bank logout now
works there thanks to GSM_DISCONNECT, after ~60 s of retries).

Regressions (2026-11-20, fifth session, after skills/slates/upgrades/item logs, Rust HG): play 22/22, fight 23/23,
pvp 37/37 with the Rust login; magic 26/27 with the Rust login (CANNOTRECALL: slimes kept hitting, the known
environment flake) and 27/27 with the C++ Login; items 41/41, trade 19/19, craft 11/11 with the C++ Login.

## Test: gate server, guilds, parties (2026-11-20, sixth session)

    restart_ab.sh [cpp]     (session scratchpad: Rust login + A (3007, all maps but elvfarm) + B (3008, elvfarm))
    hgprobe gsm LOGIN OUT.jsonl                (examples/hgprobe/social.rs)
    hgprobe guild LOGIN OUT.jsonl
    hgprobe party LOGIN MAPDATA OUT.jsonl

`gsm`: RtHg1 on A /to RtHg2 on B (WHISPERMODEON after the FINDCHARACTER round trip), a whisper through
GSM_WHISPERMSG, a '^' line through GSM_CHATMSG (both in guild 777 by SQL), /fi (PLAYERONGAME 31). `guild`: create
rtguild, admission ticket -> QUERY_JOINGUILDREQPERMISSION 16, approve -> JOINGUILDAPPROVE 28 + NEWGUILDSMAN 17,
'@' and '^' chat, REQGUILDNAMEANSWER 32, secession ticket -> approve -> DISMISSGUILDAPPROVE 38 + DISMISSGUILDSMAN
17, /ban of a non-member (CANNOTBANGUILDMAN), disband; guild / guild_member rows checked. `party`: invite ->
QUERY_JOINPARTY 12, accept -> NOTIFY_PARTY 1,1 (14) / 4,1 (22) to both / 5 list (34), PARTY_COORDS 40, '$' chat,
RtHg1 kills a slime and the idle RtHg2 gets NOTIFY_EXP, leave -> NOTIFY_PARTY 2 to both (a party of two is deleted
by the Login before its op-6 answer, which then finds nobody: same in the C++).

Results (2026-11-20, Rust login): Rust HGs gsm 5/5 (+ entry checks), guild 19/19, party 11/11; C++ HGserver copies
(A/B) with the same Rust login: the same, byte-identical messages (sizes, order, values). Differences seen: the
item list's 2 bytes (fixed earlier), NPCs / day-time / hits that depend on boot timing and the RNG. The C++ sends 10
spaces after the name of WHISPERMODEON/OFF and PLAYERNOTONGAME: now the Rust HG too (it sent one). With the C++
Login guild creation is always rejected (see the gameplay-visible fixes).
Regressions (Rust login + Rust HG): play 22/22, trade 19/19, items 41/41, pvp 37/37, magic 26/27 (the known
CANNOTRECALL flake: slimes kept hitting RtHg1).

## Test: quest scenario (2026-11-20, seventh session)

    hgprobe quest LOGIN MAPDATA OUT.jsonl       (examples/hgprobe/quest.rs)

Session 1, RtHg1 level 15 in aresden: npc 15 -> NPCTALK 64 (145), guard -> 200, the city hall offers one of
quests 1/3/5 (monster hunt in arefarm, reward chosen by dice(1,3)), accept -> QUESTCONTENTS 44 + QUESTCOUNTER 24,
the city hall again -> 101 (not done), cancel -> QUESTABORTED 8, a second accept without a talk -> nothing.
Session 2, quest 3 (30 slimes in arefarm) at 29 with the exp reward (-1, 450), placed by the arefarm slimes: one
kill -> QUESTCOUNTER 0 + QUESTCOMPLETED 8, the city hall -> QUESTREWARD 38 (4, 1, 450, the CP949 exp name,
contribution 2), then a talk again.
Results: Rust login + Rust HG 22/22; C++ Login + C++ HGserver 21/22, the one difference being the second accept
(the C++ restarts the quest: the fix above). The reward and counter messages are byte-identical.
Not covered: the item reward (gold) and its can't-carry path, GOPLACE (quests 43..47 send an aresdener to
elvine), the 35..40 abort, the quest-ID abort.

## Test: crusade scenario (2026-11-21, eighth session)

    hgprobe crusade LOGIN OUT.jsonl       (examples/hgprobe/crusade.rs; the All server, a GM account)

Session 1, RtHg1 (GM level 4, war contribution 1000) by the aresden city hall: `/crusade` -> CONSTRUCTIONPOINT
(0, 0, 1) + CRUSADE 22 (1, 0, 0, 0); fighter duty -> CRUSADE (1, 1); commander duty without a guild -> nothing;
3.5 s later the middleland map status -> MAPSTATUSLAST with the 3 mana stones (418,147) (112,174) (263,305);
aresden's for a fighter -> MAPSTATUSLAST with none. Session 2 (the crusade on): the entry sends CRUSADE (1, 1), an
energy shield generator stands in view; `/endcrusade` -> CRUSADE (0, 0, 0, 0); the next exp-stock timer ->
CRUSADE (0, 0, 1000, 0) (+5000 exp, a draw). The C++ Login refuses every GM character, so both runs use the Rust
login. Results: Rust login + Rust HG 19/19; Rust login + C++ HGserver 19/19, the middleland status byte-identical;
the C++ leaves CrusadeGUID.txt empty (the fix above), the Rust HG writes "CrusadeGUID = N / winner-side = 0".
Not covered: mana collection and the meteor strike (needs collectors built by a constructor and Mana-Stone 5000
mana), towers shooting, the hammer building, war-unit summons, guild teleport / construction locations, help,
commander points, the kill rewards, the crusade chat filter, the GSM messages between two servers.
Regressions (Rust login + Rust HG): play 22/22, fight 23/23, pvp 37/37, magic 27/27, quest 22/22, party 17/17.

## Test: heldenian scenario (2026-11-21, ninth session)

    hgprobe heldenian LOGIN OUT.jsonl       (examples/hgprobe/heldenian.rs; servers All + Events, a GM account)

Data: no server had the heldenian maps. The Events server folder (run/server/Maps/Events: inferniaA/B, maze,
druncncity, procella, abaddon, BtField, GodH, HRampart, port 3001) comes from the original server distribution
(its Maps/Events: the 10 map info .txt and GServer.cfg; the .amd
are the client's, byte-identical but GodH), with the local IPs, the HGserver.exe of All (the Towns / Middleland /
Neutrals exes are an older build the Rust login does not understand) and the map named BtField in GServer.cfg
(BTField there: the code, Teleport.cfg and the map info say BtField, so the C++ never put up its towers).
Session 1, RtHg1 (GM level 4, level 100) in CmdHall_1 (All): Gail's list -> REJECT; `/heldenian` ->
IPACCOUNTINFO, HELDENIANSTART, HELDENIANTELEPORT, CONSTRUCTIONPOINT (11000), HELDENIANCOUNT 22; Gail -> BtField for
0. Session 2 on BtField (Events): towers 87 / 89 in view, HELDENIANCOUNT 46 / 46 towers; `/endheldenian 1` ->
HELDENIANVICTORY (-1: a draw) + HELDENIANEND. Session 3, CmdHall_1: no HELDENIANSTART, Gail offers nothing.
Session 4 on BtField: a local `/heldenian` (type 2 in Rust: the GUID file keeps the last type; HRampart's 4 gates
go up) and `/endheldenian 1`.
Results: Rust login + Rust HGs 32/32; Rust login + C++ HGservers 26/32: the C++ starts only the GM's server, so
sessions 2-3 find no towers and All stays in heldenian (the relay fix); its local start (session 4) puts up the
same 46 / 46 towers.
Not covered: tower kills, flags (type 1 counts, GodH's winning flag), summon scrolls, the rewards, the deaths and
recall on the maps, the gates' fights, Gail's HRampart entries (Teleport.cfg gives them the sides
attacker / defender, which match no player in the C++ either: never offered).
Regressions (Rust login + Rust HG): play 22/22, fight 23/23, pvp 37/37, magic 27/27, quest 22/22, party 17/17, crusade 19/19.

## Test: apocalypse scenario (2026-11-21, tenth session)

    hgprobe apocalypse LOGIN OUT.jsonl       (examples/hgprobe/apocalypse.rs; servers All + Events, a GM account)

Session 1, RtHg1 (GM level 4) in aresden (All): `/apocalypse` -> IPACCOUNTINFO, APOCGATESTARTMSG, APOCGATEOPEN 24 at
0,0 (no gate on that map); the start reaches Events (GSM_BEGINAPOCALYPSE). Session 2 on inferniaA (Events): the
entry sends APOCGATECLOSE (115,105) (monsters left); `/endapocalypse` -> IPACCOUNTINFO, APOCGATEENDMSG. Session 3:
no gate state at the entry. Results: Rust login + Rust HGs 20/20; Rust login + C++ HGservers 20/20.
Not covered: an emptied map's gate opening and taking players to maze / abaddon, the boss (abaddon's
ApocalypseBossMob is 71, so a "Demon" comes, not Abaddon: the data), Abaddon's 5 / 15 minute events, the thunder,
the recall from the apocalypse maps after the end.
Regressions (Rust login + Rust HG): play 22/22, fight 23/23, pvp 37/37, magic 27/27, quest 22/22, party 17/17, crusade 19/19, heldenian 32/32.

## Test: astoria scenario (2026-11-21, eleventh session)

    hgprobe astoria LOGIN OUT.jsonl       (examples/hgprobe/astoria.rs; servers All + Events, a GM account)

Data: astoria.txt is from GitHub (isolatorhk/Helbreath.ServerFiles HGServer/MAPDATA, identical in the other forks
of this server: its astoria0 / astoria1 base teleports and spawn zones match Astoria.h), with the client's
astoria.amd, on the Events server.
Session 1, RtHg1 (GM level 4) in cityhall_1: William offers dglv2, fightzone1, bisle and no middleland (only the
last capture-the-relic winner gets it, and stats.ini has none: true of the C++ too). Session 2 on astoria at the
relic's spot: `/astoria 1` -> EVENTSTART 7 (1); OBJECTGETITEM -> ITEMOBTAINED Relic + RELICGRABBED 17; the drop ->
RELICPOSITION 10 (168,207); `/endastoria` -> CTRWINNER 7 (0, written to ../configs/stats.ini as [events] 1=0).
Results: Rust login + Rust HGs 16/16; Rust login + C++ HGservers 16/16.
Not covered: the relic in an altar and the 10-minute win, the casualties, the flags, the base teleports, the
weekend recall, the destroy-the-shield event (nothing creates shields or calls CAstoria::ShieldDestroyed in the
C++: it only ends by /endastoria, with no winner).
Regressions (Rust login + Rust HG): play 22/22, fight 23/23, pvp 37/37, magic 27/27, quest 22/22, party 17/17, crusade 19/19, heldenian 32/32, apocalypse 20/20 (fight and magic failed once on timing - a missed pick-up, the recall - and passed on the rerun).

## Test: scheduler (2026-11-21, twelfth session)

    hgprobe notices LOGIN OUT.jsonl SECS       (examples/hgprobe/scheduler.rs; the unit tests in scheduler.rs)

Schedule.cfg reads lines `event = CTR|DS|Crusade day hour minute` (day 0 = sunday). The run/server copy has
`schedule = 3 10 55` lines that no reader knows, so no event is scheduled (in the C++ too). With a test
Schedule.cfg holding a CTR event 11 minutes ahead, RtHg1 in aresden waits 150 s: Rust login + Rust HG 1
EVENTSTARTING3 7 (1); Rust login + C++ HGserver 8 identical ones (the fix above).

## Test: GM commands (2026-11-21, thirteenth session)

    hgprobe gm LOGIN OUT.jsonl       (examples/hgprobe/gm.rs; a GM account, the All server)

RtHg1 (GM level 4) and RtHg2 in aresden: /who, /afk twice, /checkrep, /setpf + /pf, /shutup, /checkip, /gns on an
NPC, /createitem Arrow 0 0 5, /polymorph Slime (seen by RtHg2 as type 10), /summonplayer, /teleport fightzone1,
/eventchat, /setattackmode 0, /eventtp, /eventreset, /closeconn. Results: Rust login + Rust HG 25/25; Rust login + C++
HGserver 25/25 (/who's count differs by the random padding).

## Test: noticement, notice.txt, energy sphere (2026-11-21, fourteenth session)

    hgprobe noticement LOGIN OUT.jsonl       (examples/hgprobe/noticement.rs; a test notice.txt with two notify_msg lines)

RtHg1 (GM level 4) on fightzone5 at (65,36): MSGID_REQUEST_NOTICEMENT size 0 -> REJECT + the Login's noticement.txt
(36 bytes: the Login sends it in text mode) + 2 zeros; the same size -> CONFIRM 6; /energysphere -> ENERGYSPHERECREATED
10 (66,37) and an Energy-Sphere (type 35) in view; a second /energysphere -> nothing; walking up to the roaming sphere and punching it -> two
OBJECTMOVE (the knock-back); a notice within 95 s. Results: Rust login + Rust HG 12/12; Rust login + C++ HGserver 12/12.
Not covered: the goal-in (NOTIFY_ENERGYSPHEREGOALIN, the contribution, the auto creation).
Regressions (Rust login + Rust HG): play 22/22, fight 23/23, pvp 37/37, magic 27/27, quest 22/22, party 17/17,
crusade 19/19, heldenian 32/32, apocalypse 20/20, astoria 16/16.

## Test: fight zone reservations and tickets (2026-11-21, fifteenth session)

    hgprobe fightzone LOGIN OUT.jsonl       (examples/hgprobe/fightzone.rs; a GM account, the All server)

RtHg1 (GM level 4, aresden, Gold 2000, FightNum / FightDate / FightTicket reset): RESERVE zone 1, or 2 when 1 is not
allowed today -> CONFIRM 1, gold 500; again -> REJECT -1; GETOCCUPYFIGHTZONETICKET -> ArenaTicket of the zone
(count 1); /reservefightzone -> -5; /reservefightzone 3 -> -4, 3; again -> -5, -3; /reservefightzone -> -4;
/getticket -> 3 ArenaTicket. Results: Rust login + Rust HG 12/12; Rust login + C++ HGserver 12/12; RESPONSE
frames byte-identical, ITEMOBTAINED identical but its last 2 bytes (uninitialised in the C++). A reservation is
saved: the next login keeps it until its date.
Not covered: the 2-hour clear, the forced recall itself, entering with a ticket.
Regressions (Rust login + Rust HG): play 22/22, fight 23/23, pvp 37/37, magic 27/27 (the recall failed once on timing,
passed on the rerun), quest 22/22, party 17/17, crusade 19/19, gm 25/25, heldenian 32/32, apocalypse 20/20, astoria 16/16.

## Test: deaths and combat leftovers (2026-11-21, sixteenth session)

    hgprobe deaths LOGIN MAPDATA OUT.jsonl       (examples/hgprobe/deaths.rs; one GM account, the All server)

1. Elo: RtHg1 (aresden, level 60) kills RtHg2 (elvine, level 40) in town: after both log out the `elo` column holds
   3 and 65533 (-3 as a WORD). 2. RtHg3 (GM level 4, level 100): `/summon Slime 1 0 7` (and 8, and 0) next to him, killed
   by hand: the SA 7 slime casts spell 30 and the SA 8 one spell 61 where it fell (COMMONTYPE_MAGIC, 18 bytes, spell + 100
   = 130 / 161), the plain one casts nothing. 3. Bows: `/summon Rabbit 24` makes a ring; with the Direction-Bow equipped
   (weapon appearance 43) a shot at a rabbit two tiles away hurts it and the rabbit in between (3 in 4); with the Fire-Bow
   (44) a shot at a rabbit two or more tiles away makes a fire (MSGID_DYNAMICOBJECT type 1 at the target) and one at an
   adjacent rabbit none. Results: Rust login + Rust HG 23/23; Rust login + C++ HGserver 23/23; the dynamic-object frames
   have the same layout in both. The rabbits wander, so the probe re-summons when no pair is in line (a retry loop, not
   a flake in the servers).
Not covered by a probe: the city win / crime counters (nothing reads them: no wire, no save), the guild war unit
construction points of KilledHandler (a crusade with guild summons), the 3 in 4 / 1 in 2 odds themselves, the Elo
formula beyond the first kill (unit test `life::tests::elo`: equal ratings 8 / -7, level factor, a stronger victim, the
log180(1) = 0 case).
Regressions (Rust login + Rust HG): play 22/22, fight 23/23, pvp 37/37, magic 27/27 (the recall failed once on timing, passed on
the rerun), quest 22/22, party 17/17, crusade 19/19, gm 25/25, fightzone 12/12, heldenian 32/32, apocalypse 20/20, astoria 16/16.

## Test: magic / items leftovers (2026-11-21, seventeenth session)

    hgprobe farming LOGIN MAPDATA OUT.jsonl       (examples/hgprobe/farming.rs; the All server)
    hgprobe recallpoint LOGIN MAPDATA OUT.jsonl   (examples/hgprobe/magic.rs)
    hgprobe polymorph LOGIN MAPDATA OUT.jsonl     (examples/hgprobe/magic.rs; a GM account, a Magic.cfg line added for the test)
    cargo test -p hg: farming::tests (the two tables with the C division, the crop produce table)

farming: RtHg1 (aresden, level 60, farming skill 60, a Hoe and four seed bags) on arefarm next to a farm tile (found in
the .amd by the 0x20 attribute): SeedBag(Ginseng) (needs 90) -> NOTIFY_AGRICULTURESKILLLIMIT 6 and the bag kept; a
watermelon on a plain tile -> AGRICULTURENOAREA 6, the bag kept; a watermelon on the farm tile -> a Crops NPC (type 64,
appearance 1 << 8 | 1, status side 0) announced by an EVENT_LOG confirm and ITEMDEPLETED_ERASEITEM; a second one on the
same tile -> AGRICULTURENOAREA, bag kept. Then the Hoe (weapon 27, combat mode) hits the crop ~60 times: three
OBJECTNULLACTIONs with the appearance 0x0102, 0x0103, 0x0103 (stages 18, 8, 1), a watermelon dropped (COMMONTYPE_ITEMDROP,
sprite 6 frame 138) and the crop removed (EVENT_LOG reject) on the last. Results: Rust login + Rust HG 15/15; Rust login
+ C++ HGserver 15/15 (the crop's appearance message is byte-identical but for the object id, direction and name). The
new NOTIFY_NOMOREAGRICULTURE / AGRICULTURESKILLLIMIT / AGRICULTURENOAREA cases of notify.rs (6 bytes) were missing before
(the first run failed on them).
recallpoint: RtHg1 (aresden, level 60, Recall) in the town sends MSGID_REQUEST_SETRECALLPNT (7 bytes, the point at offset 6)
for aresden's initial point 4 (140,205) and recalls: RESPONSE_INITDATA on aresden at (140,205) (the plain recall of a level
60 goes to arefarm); the next recall (the choice is forgotten at the map entry) goes to arefarm (50,95). Rust 8/8, C++ 8/8,
the same message sequences.
polymorph: no shipped Magic.cfg spell has type 20, so a line `magic = 5 Polymorph 20 0 6 10 1 1 1 0 0 0 0 0 0 0 0 -1 0 0`
was added to the Login's Magic.cfg for the run (restored). RtHg1 (GM level 4, spell 5) casts it on RtHg2: RtHg2 gets
NOTIFY_MAGICEFFECTON 16 (20, 1), RtHg1 sees RtHg2 as type 18 (OBJECTNULLACTION 43), a second cast on the same target is a no-effect
cast (mana spent, nothing else), after 5-6 s NOTIFY_MAGICEFFECTOFF 16 (20, 1); then the same on a `/summon Slime` (type 18 in
the 27-byte NPC form). Rust 14/14, C++ 14/14; the one difference is the fix above: the Rust HG shows the original type again
after the effect (OBJECTNULLACTION with type 1 / 10), the C++ never does.
Not covered by a probe: the 200 crops limit (NOTIFY_NOMOREAGRICULTURE), the crop's 5-minute expiry (PLANTTIME), a crop killed
with another weapon or in a crusade, the exp of a harvest (it goes to the exp stock), an out-of-array recall point, the relic
check of the drops (needs the relic on a tile a monster dies on), m_stCityStatus funds (no wire, no save), the item
events and the crusade flags (dead code / dead data, above).
Regressions (Rust login + Rust HG): play 22/22, fight 23/23, pvp 37/37, magic 27/27, quest 22/22 (the first two runs found no
slime reaching the player in 180 s: 4 FAIL; three reruns and a HEAD build run apart gave 22/22 - a timing flake of the spot
generator, not a change), party 17/17, crusade 19/19, gm 25/25, fightzone 12/12, deaths 23/23, heldenian 32/32,
apocalypse 20/20, astoria 16/16.

## Test: social / events leftovers (2026-11-21, eighteenth session)

    hgprobe leftovers LOGIN OUT.jsonl             (examples/hgprobe/leftovers.rs; the All server)
    hgprobe eventparty LOGIN MAPDATA OUT.jsonl    (the All server)
    hgprobe friends LOGIN OUT.jsonl               (two servers: restart_ae.sh, Rust or C++ HGs)
    hgprobe mobevent LOGIN MAPDATA OUT.jsonl      (the All server; see below: the special event waits 5 minutes)
    cargo test -p hg: map::tests::busiest_sector, npc::tests::special_event_tables

The server takes at most 2 connections per IP (the C++ rule), so every scenario keeps two or three players and logs the first two
out before the next ones come in.
leftovers: civil right: RtHg1 (neutral, level 10, in Aresden) -> RESPONSE_CIVILRIGHT 16 bytes, result 1, "aresden", a NULLACTION with
side 1 and, after the logout, Nation saved as aresden; asked again -> result 0, "aresden"; RtHg2 (neutral, level 4) -> result 0, "NONE".
Kennedy (gldhall_1): RtHg3 (rank 12 of rtkenn) gives its GuildSecessionTicket to Kennedy -> GIVEITEMFIN_ERASEITEM naming Kennedy,
DISMISSGUILDAPPROVE (38 bytes, rtkenn), NOTIFY_EXP 39000 -> 38700, RtHg4 (the master, same guild) gets DISMISSGUILDSMAN (17) and
the NULLACTION; RtHg4 gives his own ticket: used up, nothing else happens. Rust 30/30, C++ 30/30 (entry checks included), the notify
frames byte-identical.
eventparty: RtHg1 (GM) summons RtHg2 (non-GM) into fightzone1, they party, /eventparty 0 -> EVENTPARTY 8 bytes (1, GM byte) to both and RtHg2
is dismissed (the party of two is deleted: NOTIFY_PARTY 2 to both); RtHg2's /eventparty -> ADMINUSERLEVELLOW 6; /eventparty 1 -> 0; bare
/eventparty toggles 1, 0. Then a non-GM entering fightzone1 from the Login (RtHg3) is recalled (to arefarm). Rust 21/21, C++ 21/21 (entry checks included), same
sequences. (The first C++ run of this probe showed the recall: the non-GM P2 placed on fightzone1 by SQL was thrown out at once, which
the Rust HG did not do; that is the m_recentDCs part now ported in fightzone.rs.)
friends: RtHg1 and RtHg3 on aresden (All), RtHg2 on maze (Events). RtHg1 sends "RtHg3, RtHg2, Nobody, RtHg1": FRIENDONGAME (31 bytes) for RtHg3 and
RtHg1 at once (the C++ also finds the sender), for RtHg2 about a second later (GSM_REQUEST_FINDFRIEND through the Login, GSM_RESPONSE_FINDFRIEND
back), none for Nobody; RtHg2 asks "RtHg1,RtHg3,RtHg2": all three. Rust 16/16, C++ 16/16 (entry checks included).
mobevent: the All server with every random-mob-generator line set to `= 0 1` but aresdend1's (both the .txt and the .TXT map files: a map
left on rolls the event away from the player, which hid it in the first runs), aresdend1 with `npc-avoidrect = 1 0 0 999 999` and
maximum-object 2000 (the random generator then never fills the map: a full map, or one out of naming values, never takes the event),
the Rust HG started without --seed (a fixed seed made the roll miss). RtHg1 (GM, level 140) stands in a sector centre with open ground
(50,130) and chats every 2.5 s (the sector activity). The event comes 5 minutes after the server's start: NOTIFY_MONSTEREVENT_POSITION
11 bytes (NPC type, x, y) at the centre of the player's sector, and the master with 15 followers (the default mobevent-amount) appear around it
(13 NPCs seen in view in both). Rust 8/8 (294 and 317 s of waiting in the runs), C++ 8/8 (292 s); the NPC type is random. The probe leaves
the map files as they were (restored). m_dwSpecialEventTime starts at the start time in the C++ (dwTime of bInit): the Rust HG did
start it at 0 in the first version, which fired an event at the start; fixed.
Not covered by a probe: the type 2 event (demon / unicorn, 1 in 350), the crusade YB variant, a mob event on a map with no player activity
(the monster goes to a random place), the friends list for 13+ names, a civil right with a party (the party leave is RequestDeletePartyHandler,
tested by the party probe), Kennedy during a crusade (the ticket falls on the ground), the m_recentDCs counters (3 logouts in 10 minutes
put the player in town; the 18 s re-entry).
Regressions (Rust login + Rust HG): play 22/22, fight 23/23, pvp 37/37, magic 27/27, quest 22/22, party 17/17, crusade 19/19, gm 25/25, fightzone 12/12, deaths 23/23, farming 15/15 (14/15 once: no watermelon dropped; 15/15 on two reruns), recallpoint 8/8, heldenian 32/32, apocalypse 20/20, astoria 16/16.

## Test: batch D leftovers (2026-11-21, nineteenth session)

    hgprobe leftovers2 LOGIN OUT.jsonl            (examples/hgprobe/leftovers2.rs; the All server, Rust login)
    cargo test -p hg: gizon::tests (the stat bytes, the respec rules, the Dark Knight item names), motion::pan_tests, legion::tests

S1 RtHg1 (aresden, account cash 200): MSGID_REQUEST_LGNPTS -> NOTIFY_LGNPTS (10 bytes, 200); MSGID_REQUEST_LGNSVC (10k gold) outside the
city hall gives nothing and charges nothing. S2 RtHg1 in cityhall_1: 10k gold (ITEMOBTAINED Gold x10000, cash 199), 2 majestic points
(GIZONITEMUPGRADELEFT 2), a respec of stats that are not 607 (refused), 10 reputation, the MP 7 hat (ITEMOBTAINED LegionHat(M),
attribute 0x6100, cash 172), a town change to the same town (nothing, not charged), to Elvine (NOTIFY_RESPONSE_HUNTMODE 16 bytes "elvine",
the NULLACTION with side 2, cash 162), again (nothing), no cash (nothing); after the logout 5 cash_transactions rows, Nation elvine,
reputation 7 + 10, 2 majestic points, Gold 10005, the hat saved with attribute 24832. S3 RtHg1 as a woman with 49 of 50 bag slots:
DKFullHelm(W) fits, the next DK item and a hat are CANNOTCARRYMOREITEM (6 bytes), nothing charged. S4 RtHg2 (level 180, 150 / 150 / 100 /
45 / 100 / 62 = 607, 2 majestic points, Tremor and Fire-Wall known, HP 885): the respec of int / str / chr answers GIZONITEMUPGRADELEFT 1
then SETTING_SUCCESS (180, 149, 150, 100, 44, 100, 61) after a NOTIFY_HP 884 (the lost strength lowers the maximum), a respec at 604 is
refused, the 3 points go back with MSGID_LEVELUPSETTINGS, a byte that is no stat is refused, 3 x mag leaves no point, none is left for a
fourth; DK items 1 (DKFullHelm(M)), 13 and 10; after the logout the stats, HP 884, Tremor kept and Fire-Wall (45 int) forgotten, the
three items saved. S5 RtHg3 (GM) on aresden: panning before /obs gives nothing, after /obs directions 0, 9, 255 give nothing and three
pans answer MSGID_RESPONSE_PANNING with (x - 10, y - 7) and the direction. The map edges of the panning (x or y below 1 / above the
size) are a unit test: the C++ reads outside the map's tile array near the edges (the old WriteTileData bug), which a probe against
the C++ server could crash on.
Results (Rust login): Rust HG 71/71, C++ HG 71/71. The differences, all fixes above (the probe notes them without checking): the Rust HG
answers STATECHANGE_FAILED and sends NOTIFY_EXP after the reputation points; its RESPONSE_PANNING is 6 bytes shorter (13 bytes for no
tile in view; the C++ sends 19: the stack garbage). With the C++ Login + Rust HG the legion part and S4 pass (59 checks): the C++
Login answers LGNPTS / LGNSVC and takes the cash the same way; its GetCharacterInfo then crashes at its IsGMAccount call on the GM character of S5 (a C++ Login bug, LoginServer.cpp 2362,
not reached by any earlier probe: they ran the GM scenarios with the Rust login), which is why the GM scenarios are only run with the Rust login.
Not covered by a probe: a wrong DK item number or sex (an uninitialised name in the C++: unit test only), the guild master's 20
charisma limit, the stat limit (character-stat-limit), a town change with a party (RequestDeletePartyHandler: the party probe), the
Login's replies for a handle that has left, the cash race (several requests before the first is paid).
Regressions (Rust login + Rust HG): play 22/22, fight 23/23, pvp 37/37, magic 27/27 (26/27 three times in a row - the final recall
refused 3 times because slimes kept hitting the player - then 27/27 on the next run, and on a build of the previous commit: the timing flake
of the notes above), quest 22/22, party 17/17, crusade 19/19, gm 25/25, fightzone 12/12, deaths 23/23, farming 15/15 (14/15 once: no watermelon
dropped; 15/15 on the rerun), recallpoint 8/8, heldenian 32/32, apocalypse 20/20, astoria 16/16.

## Test: batch E leftovers (2026-11-21, twentieth session)

    hgprobe leftovers3 LOGIN OUT.jsonl            (examples/hgprobe/leftovers3.rs; the All server, Rust login)
    cargo test -p hg: reward::tests (the reward split, the hero tables), website::tests (the request bytes, a POST to a listener on 127.0.0.1,
    a refused connection)

S1 RtHg1 (level 1, strength 10: load 5500, 3000 reward gold, Gold x5 in the bag): the first COMMONTYPE_REQ_GETREWARDMONEY gives Gold x2749
(half the free load, one gold counted as 1 unit by iGetItemWeight) and NOTIFY_REWARDGOLD (10 bytes) 251, the second the other 251 and
NOTIFY_REWARDGOLD 0; after the logout reward gold 0 and 3005 Gold. S2 RtHg2 (Gold x109999 = 5499 of 5500): half the free load is 0, nothing is
given or sent and the reward gold is kept. S3 RtHg3 (aresden man, 350 EK, 25 contribution): COMMONTYPE_REQ_GETHEROMANTLE for the helm gives
aHeroHelm(M) and NOTIFY_ENEMYKILLS 200; the cap, hauberk, cape, leggings, plate (price over his EK or contribution), an index past the table and
a negative one are refused silently; saved 200 EK / 5 contribution and the helm with ITET_UNIQUE_OWNER and his character ids. S4 RtHg1
(elvine woman, 500 EK, 100 contribution): the plate is eHeroArmor(W); with 49 other items the cape (ElvineHeroCape, no contribution) takes
the last slot and the helm answers CANNOTCARRYMOREITEM (6 bytes) charging nothing (200 EK, 100 contribution saved). S5 a traveller with 500 EK and a
character with 99 EK are given nothing.
The website: the 1-minute timer was run once for real against a Python listener on 127.0.0.1:8099 (GServer.cfg with `website-script-address =
127.0.0.1/online.php`, `website-port = 8099`): the POST arrived with the C++'s headers and `code=L58sk2hK29dks&online=1`; with the lines
commented out (as the regression ran) nothing is sent. Not covered: a request to an unresolvable host (the thread logs the error), a negative
contribution (the fix above), a reward gold of 0 (the fix above: the C++ gives an empty item, so the probe cannot pass on both).
Results (Rust login): Rust HG 56/56, C++ HG 56/56 (the printed checks are identical except the number of NPCs the second player sees on entry).
Regressions (Rust login + Rust HG; the website lines of both test GServer.cfg commented out meanwhile, restored after): play 22/22, fight 23/23,
pvp 37/37, magic 27/27, quest 22/22, party 17/17, crusade 19/19, gm 25/25, fightzone 12/12, deaths 23/23, farming 15/15, recallpoint 8/8,
heldenian 32/32, apocalypse 20/20, astoria 16/16, leftovers2 71/71, leftovers3 56/56 (no timing flake this time).

## Vanilla 3.82 GM commands (2026-11-22)

Reference: the Equilibrium Project server sources (`v382/Sources/Server/Game.cpp`, the chat switch and the `AdminOrder_*`
functions) with its docs (`v382/Server/Docs`: `CommandesGM.txt`, `EP GM commands.txt`). A command counts as vanilla when
the EP docs do not list it among the Equilibrium additions and it needs no EP-only system. Admin levels are those of the
shipped `AdminSettings.cfg` (the C++ defaults of `bReadAdminSetConfigFile` are the same); the `/enableadmincommand` gate
(`m_bIsAdminCommandEnabled`) of the EP is not ported, as it is not for the existing `/summon` and `/createitem`. A user with
a level below the one needed gets NOTIFY_ADMINUSERLEVELLOW, as the existing commands (the 3.82 code returns silently for
level 0). Code: `gm.rs` (the last `impl Game` block); `Game::manual_time` (m_bManualTime) in `game.rs`.

Added (all log "GM Order(name): ..." to the Login, like the existing ones):

| Command | Level | What it does |
|---|---|---|
| `/kill name [damage]` | 2 | HP 0, `is_killed`, exchange cancelled, NOTIFY_KILLED (empty killer), OBJECTDYING (damage, weapon 1), the corpse is the tile's dead owner; no penalty, no reward. Unknown name: PLAYERNOTONGAME |
| `/revive name [damage [hp]]` | 2 | HP = hp (1 by default, cut to the maximum), alive, NOTIFY_HP, OBJECTDAMAGE; no re-entry |
| `/summondemon` | 2 | one Demon with a random special ability (rand % 9) where the GM stands |
| `/summondeath` | 3 | the same with a Wyvern |
| `/unsummondemon` | 2 | every Demon (type 31) of the server falls |
| `/clearnpc`, `/unsummonnpc` | 3 | every NPC of the GM's map is killed and deleted, except the summoned ones, the merchants and the gates (types 15 19 20 24 25 26 67 68 69 90 91) |
| `/setzerk 1/0`, `/setfreeze 1/0` | 2 | the berserk / frozen aura (status bit) on the GM himself; the berserk effect value is the Value4 of Magic.cfg entry 32 (0), so the aura is only visual, as in the 3.82 code |
| `/safe`, `/peace`, `/unsafe`, `/war` | 2 | attacks off / on in the GM's map (any map, unlike our `/setattackmode`, which is for fight zones and level 3), NOTIFY_GLOBALATTACKMODE to its players |
| `/weather type seconds` | 3 | that weather (1..3 rain, 4..6 snow, 0 none) on every map that has none; NOTIFY_WHETHERCHANGE to the players of each |
| `/time 0/1/2` | 3 | 1 day, 2 night held against the 3 s day/night check (`manual_time`), 0 back to the clock; NOTIFY_TIMECHANGE to every player of a map that is not fixed-day |
| aliases `/push`, `/cleanmap`, `/target`, `/callguard`, `/clearboss` | those of /send, /clearmap, /tgt, /attack, /unsummonboss | the other names the 3.82 code accepts |

Fixes and deviations from the 3.82 code (all marked `// fix:` in the Rust):
- `/revive` called SetDeadOwner after ClearOwner, so a revived player stayed a corpse for the map (nobody could target him):
  he is the live owner of his tile again. (gameplay-visible)
- `/weather` multiplied the time by 1000 inside the map loop (the 2nd map with no weather got 1000 times the time, the 3rd a
  million...): once. The type is cut to 0..6 (what the client knows).
- `/time 0` logged the previous GM line (stale buffer): it logs "Disable force time mode".
- `/unsummondemon` gave the kill (exp, drops) to the GM: the demons are marked as summoned first, as the existing `/unsummonboss`
  does (the 3.82 `/clearnpc` and `/unsummonboss` have an `m_bIsUnsummoned` flag for that; `/unsummondemon` forgot it).
- `/kill` and `/revive` ignore a player who has not finished entering the world (no map position yet); the C++ did not check.
- The 3.82 `/safe` logged even when the level was too low: here only when it ran.
- `/clearnpc` here uses `is_summoned = true` (no exp, no drop) in place of `m_bIsUnsummoned`; the Rust `delete_npc` still rolls its
  60 / 100000 slate as for any deleted NPC.

Skipped, with the reason:
- Exists already under another name or number: `/beginheldenian`, `/endheldenian` (= `/heldenian`, `/endheldenian`),
  `/begincrusadetotalwar`, `/endcrusadetotalwar` (= `/crusade`, `/endcrusade`: GlobalStartCrusadeMode / ManualEndCrusadeMode, no argument; the 3.82
  ones also need `/enableadmincommand`), `/monstercount` (= `/mcount`: NOTIFY_MONSTERCOUNT has no case in our SendNotifyMsg, so nothing is sent in either),
  `/setattackmode` (stays, see `/safe`), `/clearmap`, `/send`, `/summonplayer`, `/gns`, `/disconnectall`, ...
- Equilibrium-only (listed in `EP GM commands.txt`, or built on an EP system): `/setxprate`, `/setbossrate`, `/setextradrop` (map xp /
  drop settings), `/setpkmode`, `/setmaptype`, `/setdroptype`, `/setmapmobnum`, `/popoenabled` (map modes), `/thunder`, `/gate`,
  `/gateto`, `/forcegate`, `/forcetp` (EP equilibrium-mode map hosting), `/createtoken`, `/itemeventnow` (EP item events),
  `/summonboss`, `/cleanbumps`, `/logpits`, `/monitor` (anti-hack), `/checkstats`, `/checkstatus` (calls the EP /checkstats),
  `/readgizon`, `/checkgizon`, `/getgizonlevel` (EP gizon points), `/savechar`, `/readmapinfo`, `/checkmapinfo`,
  `/getactivegservers`, `/mobilise`, `/summons`, `/unpretend`, `/plraider`, `/enableadmincommand`, `/restartthisserverrightnow`,
  `/start`, `/reloadmap`, `/test`, the avatar war commands, `/version` (prints "Equilibrium Project by Snoopy81").
- Needs something our servers lack: `/summonall`, `/summonguild` need a gate-server relay (GSM_REQUEST_SUMMONALL is not in our
  NetMessages.h; GSM_REQUEST_SUMMONGUILD is defined but neither the C++ HG nor the Rust one handles it), and `/summonguild` also
  charges `m_iSummonGuildCost` (EP setting) from a non-GM guild master.
- Unsure, left out: `/storm` (Snoopy's rewrite of the old one: ~200 lines of spell effects, weather bonus and a self-ice rule, gated by
  `/enableadmincommand`; port it as its own task if wanted), `/setstatus` (in the 3.82 code it tests the first character of the command
  word, `'/'`, instead of the argument, so it does nothing and only logs).

Test: `hgprobe gm382 LOGIN OUT.jsonl` (`examples/hgprobe/gm382.rs`, Rust only: the C++ HG has none of these). RtHg1 (GM 4) +
RtHg2 (player), then RtHg3 (GM 2) once RtHg2 left (the HG takes two clients per IP): the level gates of a player and of a level 2 GM,
/weather (snow to both, a second /weather does nothing while it lasts, a missing time is ignored), /kill (KILLED 26 with an empty
killer, OBJECTDYING with the damage seen by both, unknown name) and /revive (HP 50, OBJECTDAMAGE, HP cut to the maximum),
/summondeath + /clearnpc (the Wyvern dies and is deleted, McGaffin stays), /safe + /war, /setzerk + /setfreeze (the status bits seen
by RtHg2, on and off), /time (the forced value holds 7 s against the 3 s check, back to the clock with 0), then the level 2 GM's
/summondemon + /unsummondemon, /peace, /unsafe. Run it within a minute of the HG start (the weather processor of that HG may give the
map a weather later, and `/weather` does nothing on a map that has one).

Regressions (Rust login + Rust HG, after this change): play 22/22, fight 23/23, pvp 37/37, magic 27/27, quest 22/22, party 17/17, crusade 19/19, gm 25/25, fightzone 12/12, deaths 23/23, farming 15/15, recallpoint 8/8, heldenian 32/32, apocalypse 20/20, astoria 16/16, leftovers2 71/71, leftovers3 56/56, gm382 54/54 (no timing flake). Unit tests: 79 hg + 21 login.

## Next steps

1. (done) The GM layers and the leftovers lists: what is not ported is either dead code ("Not ported on purpose") or listed under "Still not ported".
2. (done) Owner decisions on the gate-server / guild / party fixes: all eight kept, party exp divided.
3. Data issues for the owner (not code): the item config (Login/Config/Items) still numbers skills the old way
   (FishingRod related skill 1 = agility now, PickAxe 0, the manuals: FishingManual -> skill 1, MiningManual -> 0,
   PretendCorpseManual -> 19, out of range), so fishing with a rod never yields a fish and most manuals teach the
   wrong skill, in both servers.
   Skill 10 in the player data (owner's decision, done): pretend corpse is persisted by the Rust login + Rust HG
   only. It rides in the block at the natural b = 10 slots: mastery byte 608 (598 + 10) and SSN dword 693..696
   (653 + 40), inside the gaps 608..652 and 693..750 that no C++ side reads or writes (HG _bDecode /
   _iCompose and Login GetCharacterInfo / SaveCharacter touch 0..597 profile included, 598..607, 653..692 and
   751+ only; the C++ HG zeroes G_cData50000 before composing, so it sends 0 there). The Rust login keeps its own
   `skill` row SkillID 10 (new characters get it at 0; an existing character gets it at its first Rust load),
   loads it apart from the 0..9 query (which now skips SkillID 10, so rows 0..9 sent are as before) and updates
   it on save. The C++ Login's `LIMIT 10` query has no ORDER BY: it takes the first 10 rows in insertion order,
   and the servers only ever insert row 10 after 0..9, so it never sees row 10 (a row 0..9 re-inserted by hand
   after row 10 would be dropped by it: update rows in place). Mixes: C++ Login + any HG: skill 10 lost as
   before; C++ HG + Rust login: the HG sends 0, so the Rust login saves row 10 as 0/0.
4. (done) The social / events leftovers: friends list, civil right, /eventparty, Kennedy, the special mob events. The item events and the other
   leftovers are dead code ("Not ported on purpose").
   (done, batch D) the client messages StateChangeHandler, GetDKItemHandler, the panning request and the Login relays LGNPTS / LGNSVC.
   (done, batch E) GetRewardMoneyHandler, GetHeroMantleHandler, UpdateWebsiteStats: no live C++ HG code is left unported; everything else
   of the HG is ported or documented as dead.
5. (done, kept) The owner's decision on the gameplay-visible fixes of the skill / slate / upgrade layer: all six kept (2026-11-20, confirmed 2026-11-21).
6. (done, kept) The owner's decision on the batch D leftovers (the list above): the added STATECHANGE_FAILED / SPELL_SKILL / EXP notifies, and whether
   GetDKItemHandler should check the level cap.
7. (done, kept) The owner's decision on the batch E leftovers (the list above): the signed contribution check of the hero items, and the empty reward
   gold request.

## Rust only: in-process bots (2026-11-22, `bots.rs`, `Bots.cfg`)

No C++ counterpart. Off unless `Bots.cfg` (next to `Settings.cfg`; `hg --bots-sample` prints the documented sample, every key
with its default) has `bot-count` > 0, or `hg --bots N --bots-login HOST:PORT [--bots-cfg FILE]` says so (the Login's client TCP
port, `login --tcp-port`). A task per bot creates the account `<prefix>NN` and character if missing (through the Login), does the
ENTERGAME, and hands the game a fake connection through `Ev::Accept` / `Ev::Read` (its "IP" is `bot-N`: bAccept allows 2
connections per IP). The brain (`Game::bots_process`, after `msg_process`) reads the world directly and acts by pushing the frames a
client would send into `Game::queue`, so every handler and anti-hack check applies to a bot as to a player (`step-ms` / `attack-ms`
cannot go under the speed-hack limits of `motion_move` / `motion_attack`: 150 / 220 ms).

Behaviour (modes `Hunt`, `Rest`, `Travel`):
- equips the best weapon and fills empty armour slots from the bag (the server's refusals are not retried);
- A* (`find_path`) to the nearest hostile monster within `sight`, combat mode on, hits until it dies; gives a target up after
  `giveup-hits` hits that leave it above `giveup-percent` HP (2 minutes); wanders within `wander-radius` of its home tile;
- below `potion-percent` HP drinks a red potion (any `ITEMEFFECTTYPE_HP` item); with none left and below `recall-percent` reads a
  recall scroll; below `tired-percent` it only fights what is within 2 tiles;
- away from its home map (recall, death restart): `Rest` until `rest-percent` HP, then `Travel`: BFS over the `teleport-loc` links
  of the maps this server runs, A* to the teleport tile, `REQUEST_TELEPORT`, and so on until home (`auto-return`); a route that needs
  another game server is not followed ("no teleport route" in the log);
- `REQUEST_RESTART` after dying; a status line per bot every `status-log-ms`.
- loot (`loot`, `loot-radius`): between two fights it walks to the closest tile with items and sends the get-item command until the
  tile is empty (a tile it cannot clear in 4 tries or reach is left alone for 2 minutes);
- shopping (`shop`, `shop-*`, `sell-loot`): with fewer than `shop-potion-min` potions / `shop-scroll-min` recall scrolls (or `shop-bag-slots`
  items in the bag) and gold plus loot value for at least one potion, it goes to the nearest `gshop*` map through the teleport tiles
  (`is_on_shop` is set by the map name), sells the loot (shop category 11-50: `COMMONTYPE_REQ_SELLITEMCONFIRM`, one item per
  `0.6 s`), buys potions and scrolls up to the targets (`COMMONTYPE_REQ_PURCHASEITEM`), then `Travel`s home; a trip is not
  repeated before `shop-cooldown-ms`, and one that lasts 3 minutes is dropped.
- surplus gear: on the shop trip it also sells gear (shop category 1-10) it will never wear: a weapon no better than the one in
  hand (`weapon_score` = twice the average damage, so a 1d6 beats a 1d5), a piece for a slot already filled, or one the server
  refused to equip (`is_surplus`); gear for an empty slot stays in the bag for `bot_equip`.
  `is_loot` (shop category 11-50) leaves out item type 1: armour (categories 11-13), cloaks and necklaces (46) are gear, never loot
  (the server does not refuse to sell a worn piece), and gear for a slot the bot never fills (necklace, cloak, ring, pants) is surplus.
- repairs (`repair`, `repair-percent`): a trip also starts when a worn piece, or one that broke while worn (the server unequips it
  at 0 endurance), is under `repair-percent` % of its endurance and the bot has the gold; after the purchases it sends
  `COMMONTYPE_REQ_REPAIRITEMCONFIRM` for each such piece (the server only wants the bot on a shop map) and wears the repaired
  weapon again. Neutral characters never wear gear out (`is_neutral` in `combat.rs`), so this only matters after joining a town.
- hunting grounds (`roam`, `roam-idle-ms`, `hunt-level-step`): a new character spawns far from any monster (`default`:
  the slime spots are 40-80 tiles away, sight is 12), so with nothing to fight in sight for `roam-idle-ms` the bot picks one of
  the 5 nearest live monsters it can beat on its map (or, if the map has none, a random one on the nearest map of this server
  that has some, through the teleport tiles), makes that tile its new home and `Travel`s there; every 20 s at most. The bot
  only fights monsters whose HitDice is at most `2 + level / hunt-level-step` (level 1: slimes; 12: orcs and amphis; 24:
  scorpions; 0 = any); a monster it cannot beat is left alone in sight and in the ground search.
Not yet: spells, fleeing on foot, parties, PvP.

Test (isolated Login on other ports + a copy of `Maps/All`, same MySQL): `login --tcp-port 4100 --ws-port 4102 --gs-port 5757
--no-db-maintenance`, `hg --login 127.0.0.1:5757` with a `Bots.cfg` (`bot-count = 1`, `bot-login-address = 127.0.0.1:4100`) and a
`GServer.cfg` with another `game-server-port`; run once to create the character, stop, then `UPDATE char_database SET
MapLoc='aresden', LocX=115, LocY=58, Level=30, Exp=31209, HP=60 ...` and run again. Seen: Dagger equipped, potions drunk at 60/170,
slimes in 2 hits, a death by a scorpion -> restart in `default` -> Travel through `arefarm` back to aresden -> hunting again.
Shop run (scratch `Settings.cfg` with `primary-drop-rate = 8000` so that monsters drop things, `shop-potion-min = 4`,
`shop-cooldown-ms = 20000`): gold and potions picked up, trip to `gshop_1`, 8 red potions bought for 40 gold, back home and hunting;
with `shop-bag-slots = 20` a second trip sold the loot (+76 gold) and bought a recall scroll.
Six bots at once (`hg --bots 6`, same scratch setup, all set to Level 30 at aresden 115,58): two runs of about 8 minutes without a kick, bots shared the
slime field, looted the same tiles without trouble, each made its own trips to `gshop_1` (about 1 minute each way) and sold potions
of other kinds and surplus gear (a `ShortSword` equipped over the `Dagger`, then the dagger sold on the next trip); the debug `hg`
used about 15 s of CPU in 7 minutes.
Repair run (`Nation='Aresden'`, dagger `LifeSpan=20`, `shop-potion-min = 1`): trip to `gshop_1`, spare dagger sold, 7 potions bought,
`repairs Dagger (slot 0, 11 gold)`, back home.
Hunting ground run (3 bots, prefix `tst`, created by the bots themselves at the `default` spawn, level 1): ~8 s without a monster, a
ground at `default(100,85)`, 20 s walk, slimes killed in 1-3 hits; started inside `gshop_1`, they left through the teleports and
were killing at `aresden` 40 s later.
Remove the rows afterwards (`char_database`, `item`, `bank_item`, `skill`, `account_database` for the bot names).
