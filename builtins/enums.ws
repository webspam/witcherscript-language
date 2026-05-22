// Built-in: engine-provided enums that have no declaration in user code or shipped scripts.
enum EAIAreaSelectionMode { EAIASM_GuardArea }
enum EAIAttitude { AIA_Friendly, AIA_Hostile, AIA_Neutral }
enum EActorActionType { ActorAction_Exploration, ActorAction_None }
enum EAnimationEventType { AET_Duration, AET_DurationEnd, AET_DurationStart, AET_DurationStartInTheMiddle, AET_Tick }
enum EAnimationManualSyncType { AMST_SyncBeginning, AMST_SyncMatchEvents }
enum EArbitratorPriorities { BTAP_AboveCombat, BTAP_Emergency }
enum EAsyncCheckResult { ASR_ReadyTrue }
enum EAsyncTestResult { EAsyncTastResult_Failure, EAsyncTastResult_Invalidated, EAsyncTastResult_Pending, EAsyncTastResult_Success }
enum EAttackDirection { AD_Back, AD_Front, AD_Left, AD_Right }
enum EAttackDistance { ADIST_Large, ADIST_Medium, ADIST_Small }
enum EBTNodeStatus { BTNS_Active, BTNS_Completed, BTNS_Failed }
enum EBaseCharacterStats { BCS_Air, BCS_Essence, BCS_Focus, BCS_Morale, BCS_Panic, BCS_PanicStatic, BCS_Stamina, BCS_SwimmingStamina, BCS_Toxicity, BCS_Undefined, BCS_Vitality }
enum EBatchQueryState { BQS_NotReady, BQS_Processed }
enum EBufferActionType { EBAT_CastSign, EBAT_Ciri_Counter, EBAT_Ciri_Dodge, EBAT_Ciri_SpecialAttack, EBAT_Ciri_SpecialAttack_Heavy, EBAT_Dodge, EBAT_Draw_Silver, EBAT_Draw_Steel, EBAT_EMPTY, EBAT_HeavyAttack, EBAT_ItemUse, EBAT_LightAttack, EBAT_Parry, EBAT_Roll, EBAT_Sheathe_Sword, EBAT_SpecialAttack_Heavy, EBAT_SpecialAttack_Light }
enum ECharacterDefenseStats { CDS_BleedingRes, CDS_BludgeoningRes, CDS_BurningRes, CDS_DoTBleedingDamageRes, CDS_DoTBurningDamageRes, CDS_DoTPoisonDamageRes, CDS_ElementalRes, CDS_FireRes, CDS_ForceRes, CDS_FreezeRes, CDS_FrostRes, CDS_None, CDS_PhysicalRes, CDS_PiercingRes, CDS_PoisonRes, CDS_RendingRes, CDS_ShockRes, CDS_SlashingRes, CDS_WillRes }
enum ECharacterPhysicsState { CPS_Swimming }
enum ECollisionSides { CS_BACK, CS_BACK_LEFT, CS_BACK_RIGHT, CS_CENTER, CS_FRONT, CS_FRONT_LEFT, CS_FRONT_RIGHT, CS_LEFT, CS_RIGHT }
enum ECombatActionType { CAT_Crossbow }
enum ECombatTargetSelectionSkipTarget { CTSST_SKIP_ALWAYS, CTSST_SKIP_IF_THERE_ARE_OTHER_TARGETS }
enum EComboAttackType { ComboAT_Directional, ComboAT_Normal }
enum EDialogActionIcon { DialogAction_ARMORER, DialogAction_AUCTION, DialogAction_AXII, DialogAction_BET, DialogAction_BRIBE, DialogAction_CONTENT_MISSING, DialogAction_EXIT, DialogAction_FAST_TRAVEL, DialogAction_GAME_CARDS, DialogAction_GAME_DAGGER, DialogAction_GAME_DICES, DialogAction_GAME_DRINK, DialogAction_GAME_WRESTLE, DialogAction_GETBACK, DialogAction_HAIRCUT, DialogAction_HOUSE, DialogAction_MONSTERCONTRACT, DialogAction_NONE, DialogAction_RUNESMITH, DialogAction_SHAVING, DialogAction_SHOPPING, DialogAction_SMITH, DialogAction_TEACHER }
enum EDifficultyMode { EDM_Easy, EDM_Hard, EDM_Hardcore, EDM_Medium, EDM_NotSet }
enum EDismountType { DT_instant, DT_normal, DT_ragdoll, DT_shakeOff }
enum EEntityGameplayEffectFlags { EGEF_CatViewHiglight }
enum EExplorationType { ET_Boat_B, ET_Boat_Enter_From_Beach, ET_Boat_P, ET_Boat_Passenger_B, ET_Fence_OneSided, ET_Ladder, ET_Ledge }
enum EFinisherSide { FinisherLeft, FinisherRight }
enum EFocusModeVisibility { FMV_Clue, FMV_Interactive, FMV_None }
enum EGlobalEventCategory { GEC_Empty, GEC_Fact, GEC_ScriptsCustom0, GEC_ScriptsCustom1, GEC_ScriptsCustom2, GEC_ScriptsCustom3, GEC_ScriptsCustom4, GEC_ScriptsCustom5, GEC_ScriptsCustom6, GEC_ScriptsCustom7, GEC_ScriptsCustom8, GEC_Tag }
enum EGlobalEventType { GET_Unknown }
enum EGwintAggressionMode { EGAM_Defensive }
enum EGwintDifficultyMode { EGDM_Easy }
enum EInputKey { IK_1, IK_3, IK_A, IK_Alt, IK_Backspace, IK_C, IK_CapsLock, IK_Ctrl, IK_D, IK_Delete, IK_Down, IK_E, IK_End, IK_Enter, IK_Escape, IK_Execute, IK_F, IK_Home, IK_Insert, IK_LControl, IK_LShift, IK_Left, IK_LeftMouse, IK_MiddleMouse, IK_Mouse4, IK_Mouse5, IK_Mouse6, IK_Mouse7, IK_Mouse8, IK_MouseWheelDown, IK_MouseWheelUp, IK_None, IK_NumLock, IK_NumMinus, IK_NumPad0, IK_NumPad1, IK_NumPad2, IK_NumPad3, IK_NumPad4, IK_NumPad5, IK_NumPad6, IK_NumPad7, IK_NumPad8, IK_NumPad9, IK_NumPeriod, IK_NumPlus, IK_NumSlash, IK_NumStar, IK_PS4_OPTIONS, IK_PS4_TOUCH_PRESS, IK_Pad_A_CROSS, IK_Pad_B_CIRCLE, IK_Pad_Back_Select, IK_Pad_DigitDown, IK_Pad_DigitLeft, IK_Pad_DigitRight, IK_Pad_DigitUp, IK_Pad_LeftAxisY, IK_Pad_LeftShoulder, IK_Pad_LeftThumb, IK_Pad_LeftTrigger, IK_Pad_RightShoulder, IK_Pad_RightThumb, IK_Pad_RightTrigger, IK_Pad_Start, IK_Pad_X_SQUARE, IK_Pad_Y_TRIANGLE, IK_PageDown, IK_PageUp, IK_Pause, IK_Print, IK_PrintScrn, IK_RControl, IK_RShift, IK_Right, IK_RightMouse, IK_ScrollLock, IK_Select, IK_Separator, IK_Shift, IK_Space, IK_Tab, IK_Up, IK_V, IK_X, IK_Z }
enum EInteractionPriority { IP_Max_Unpushable, IP_NotSet, IP_Prio_0, IP_Prio_12, IP_Prio_3, IP_Prio_5 }
enum EInventoryEventType { IET_ItemQuantityChanged, IET_ItemRemoved }
enum EJournalStatus { JS_Active, JS_Failed, JS_Inactive, JS_Success }
enum ELoadGameResult { LOAD_Error, LOAD_Initializing, LOAD_Loading, LOAD_MissingContent, LOAD_NotInitialized }
enum EMinigameState { EMS_End_PlayerForfeited, EMS_End_PlayerLost, EMS_End_PlayerWon, EMS_None }
enum EMountType { MT_instant, MT_normal }
enum EMoveFailureAction { MFA_EXIT }
enum EMoveType { MT_AbsSpeed, MT_FastRun, MT_Run, MT_Sprint, MT_Walk }
enum EMovementAdjustmentNotify { MAN_LocationAdjustmentReachedDestination }
enum ENPCGroupType { ENGT_Commoner, ENGT_Enemy, ENGT_Guard, ENGT_Quest }
enum ENavigationReachabilityTestType { ENavigationReachability_Any }
enum ENewGamePlusStatus { NGP_CantLoad, NGP_ContentRequired, NGP_InternalError, NGP_Invalid, NGP_RequirementsNotMet, NGP_Success, NGP_TooOld }
enum ENpcStance { NS_Fly, NS_Guarded, NS_Normal, NS_Retreat, NS_Strafe, NS_Swim, NS_Wounded }
enum EOrientationTarget { OT_Actor, OT_Camera, OT_CameraOffset, OT_CustomHeading, OT_None, OT_Player }
enum EPersistanceMode { PM_DontPersist, PM_Persist }
enum EPropertyAnimationOperation { PAO_Play, PAO_Rewind, PAO_Stop }
enum EPropertyCurveMode { PCM_Backward, PCM_Forward }
enum EQuestManageFastTravelOperation { QMFT_EnableAndShow, QMFT_EnableOnly, QMFT_ShowOnly }
enum ER4CommonStats { CS_DIFFICULTY_LVL, CS_TOXICITY, CS_VITALITY }
enum ER4TelemetryEvents { TE_ELIXIR_USED, TE_FIGHT_ENEMY_DIES, TE_FIGHT_ENEMY_GETS_HIT, TE_FIGHT_HERO_GETS_HIT, TE_FIGHT_HERO_THROWS_BOMB, TE_FIGHT_PLAYER_ATTACKS, TE_FIGHT_PLAYER_DIES, TE_FIGHT_PLAYER_USE_SIGN, TE_HERO_CASH_CHANGED, TE_HERO_EXP_EARNED, TE_HERO_FOCUS_OFF, TE_HERO_FOCUS_ON, TE_HERO_GWENT_MATCH_ENDED, TE_HERO_GWENT_MATCH_STARTED, TE_HERO_LEVEL_UP, TE_HERO_MUTAGEN_USED, TE_HERO_SKILL_POINT_EARNED, TE_HERO_SKILL_UP, TE_INV_ITEM_BOUGHT, TE_INV_ITEM_DROPPED, TE_INV_ITEM_EQUIPPED, TE_INV_ITEM_PICKED, TE_INV_ITEM_SOLD, TE_INV_ITEM_UNEQUIPPED, TE_INV_QUEST_COMPLETED, TE_ITEM_COOKED, TE_STATE_AIM_THROW, TE_STATE_COMBAT, TE_STATE_DIALOG, TE_STATE_EXPLORING, TE_STATE_HORSE_RIDING, TE_STATE_SAILING, TE_STATE_SWIMMING }
enum ERidingManagerTask { RMT_DismountHorse, RMT_None }
enum ESaveGameType { SGT_AutoSave, SGT_CheckPoint, SGT_ForcedCheckPoint, SGT_Manual, SGT_QuickSave }
enum ESessionRestoreResult { RESTORE_DLCRequired, RESTORE_DataCorrupted, RESTORE_InternalError, RESTORE_MissingContent, RESTORE_NoGameDefinition, RESTORE_WrongGameVersion }
enum EShowFlags { SHOW_AI, SHOW_Containers, SHOW_Exploration }
enum ESpawnTreeSpawnVisibility { STSV_SPAWN_HIDEN }
enum EStorySceneSignalType { SSST_Accept, SSST_Highlight, SSST_Skip }
enum ESyncRotationUsingRefBoneType { SRT_TowardsOtherEntity }
enum ETickGroup { TICK_Main, TICK_PrePhysics }
enum EUsableItemType { UI_Horn, UI_Mask, UI_Meteor }
enum EVehicleMountStatus { VMS_dismountInProgress, VMS_dismounted, VMS_mountInProgress, VMS_mounted }
enum EVehicleMountType { VMT_ApproachAndMount, VMT_ImmediateUse, VMT_MountIfPossible, VMT_TeleportAndMount }
enum EVehicleSlot { EVS_driver_slot, EVS_passenger_slot }
enum EVehicleType { EVT_Boat, EVT_Horse, EVT_Undefined }
enum EWitcherSwordType { WST_Silver, WST_Steel }
enum EWoundTypeFlags { WTF_All, WTF_Cut, WTF_Explosion, WTF_Frost }
enum eGwintFaction { GwintFaction_Neutral, GwintFaction_Nilfgaard, GwintFaction_NoMansLand, GwintFaction_NothernKingdom, GwintFaction_Scoiatael, GwintFaction_Skellige }
enum eQuestType { Chapter, MonsterHunt, Side, Story, TreasureHunt }
