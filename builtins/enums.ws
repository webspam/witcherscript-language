// Built-in: engine-provided enums that have no declaration in user code or shipped scripts.
enum EAIAreaSelectionMode { EAIASM_GuardArea = 1 }
enum EAIAttitude { AIA_Friendly = 0, AIA_Hostile = 2, AIA_Neutral = 1 }
enum EActorActionType { ActorAction_Exploration = 128, ActorAction_None = 0 }
enum EAnimationEventType { AET_Duration = 4, AET_DurationEnd = 3, AET_DurationStart = 1, AET_DurationStartInTheMiddle = 2, AET_Tick = 0 }
enum EAnimationManualSyncType { AMST_SyncBeginning = 0, AMST_SyncMatchEvents = 2 }
enum EArbitratorPriorities { BTAP_AboveCombat = 86, BTAP_Emergency = 50 }
enum EAsyncCheckResult { ASR_ReadyTrue = 1 }
enum EAsyncTestResult { EAsyncTastResult_Failure = 0, EAsyncTastResult_Invalidated = 3, EAsyncTastResult_Pending = 2, EAsyncTastResult_Success = 1 }
enum EAttackDirection { AD_Back = 3, AD_Front = 0, AD_Left = 1, AD_Right = 2 }
enum EAttackDistance { ADIST_Large = 2, ADIST_Medium = 1, ADIST_Small = 0 }
enum EBTNodeStatus { BTNS_Active = 0, BTNS_Completed = 2, BTNS_Failed = 1 }
enum EBaseCharacterStats { BCS_Air = 6, BCS_Essence = 1, BCS_Focus = 4, BCS_Morale = 5, BCS_Panic = 7, BCS_PanicStatic = 8, BCS_Stamina = 2, BCS_SwimmingStamina = 9, BCS_Toxicity = 3, BCS_Undefined = 10, BCS_Vitality = 0 }
enum EBatchQueryState { BQS_NotReady = 1, BQS_Processed = 2 }
enum EBufferActionType { EBAT_CastSign = 3, EBAT_Ciri_Counter = 12, EBAT_Ciri_Dodge = 13, EBAT_Ciri_SpecialAttack = 10, EBAT_Ciri_SpecialAttack_Heavy = 11, EBAT_Dodge = 6, EBAT_Draw_Silver = 15, EBAT_Draw_Steel = 14, EBAT_EMPTY = 0, EBAT_HeavyAttack = 2, EBAT_ItemUse = 4, EBAT_LightAttack = 1, EBAT_Parry = 5, EBAT_Roll = 9, EBAT_Sheathe_Sword = 16, EBAT_SpecialAttack_Heavy = 8, EBAT_SpecialAttack_Light = 7 }
enum ECharacterDefenseStats { CDS_BleedingRes = 2, CDS_BludgeoningRes = 13, CDS_BurningRes = 10, CDS_DoTBleedingDamageRes = 18, CDS_DoTBurningDamageRes = 16, CDS_DoTPoisonDamageRes = 17, CDS_ElementalRes = 15, CDS_FireRes = 4, CDS_ForceRes = 7, CDS_FreezeRes = 8, CDS_FrostRes = 5, CDS_None = 0, CDS_PhysicalRes = 1, CDS_PiercingRes = 12, CDS_PoisonRes = 3, CDS_RendingRes = 14, CDS_ShockRes = 6, CDS_SlashingRes = 11, CDS_WillRes = 9 }
enum ECharacterPhysicsState { CPS_Swimming = 3 }
enum ECollisionSides { CS_BACK = 2, CS_BACK_LEFT = 7, CS_BACK_RIGHT = 6, CS_CENTER = 8, CS_FRONT = 0, CS_FRONT_LEFT = 4, CS_FRONT_RIGHT = 5, CS_LEFT = 3, CS_RIGHT = 1 }
enum ECombatActionType { CAT_Attack = 0, CAT_CastSign = 6, CAT_CiriDodge = 12, CAT_Crossbow = 10, CAT_Dodge = 2, CAT_ItemThrow = 4, CAT_Parry = 9, CAT_PreAttack = 8, CAT_Roll = 3, CAT_SpecialAttack = 1 }
enum ECombatTargetSelectionSkipTarget { CTSST_SKIP_ALWAYS = 0, CTSST_SKIP_IF_THERE_ARE_OTHER_TARGETS = 1 }
enum EComboAttackType { ComboAT_Directional = 1, ComboAT_Normal = 0 }
enum EDialogActionIcon { DialogAction_ARMORER = 4194304, DialogAction_AUCTION = 268435456, DialogAction_AXII = 2, DialogAction_BET = 65536, DialogAction_BRIBE = 8, DialogAction_CONTENT_MISSING = 4, DialogAction_EXIT = 8192, DialogAction_FAST_TRAVEL = 33554432, DialogAction_GAME_CARDS = 67108864, DialogAction_GAME_DAGGER = 1048576, DialogAction_GAME_DICES = 128, DialogAction_GAME_DRINK = 524288, DialogAction_GAME_WRESTLE = 512, DialogAction_GETBACK = 64, DialogAction_HAIRCUT = 16384, DialogAction_HOUSE = 16, DialogAction_MONSTERCONTRACT = 32768, DialogAction_NONE = 1, DialogAction_RUNESMITH = 8388608, DialogAction_SHAVING = 134217728, DialogAction_SHOPPING = 2048, DialogAction_SMITH = 2097152, DialogAction_TEACHER = 16777216 }
enum EDifficultyMode { EDM_Easy = 1, EDM_Hard = 3, EDM_Hardcore = 4, EDM_Medium = 2, EDM_NotSet = 0 }
enum EDismountType { DT_instant = 8, DT_normal = 1, DT_ragdoll = 4, DT_shakeOff = 2 }
enum EEntityGameplayEffectFlags { EGEF_CatViewHiglight = 2 }
enum EExplorationType { ET_Boat_B = 9, ET_Boat_Enter_From_Beach = 11, ET_Boat_P = 10, ET_Boat_Passenger_B = 15, ET_Fence_OneSided = 13, ET_Ladder = 1, ET_Ledge = 14 }
enum EFinisherSide { FinisherLeft = 0, FinisherRight = 1 }
enum EFocusModeVisibility { FMV_Clue = 2, FMV_Interactive = 1, FMV_None = 0 }
enum EGlobalEventCategory { GEC_Empty = 0, GEC_Fact = 3, GEC_ScriptsCustom0 = 4, GEC_ScriptsCustom1 = 5, GEC_ScriptsCustom2 = 6, GEC_ScriptsCustom3 = 7, GEC_ScriptsCustom4 = 8, GEC_ScriptsCustom5 = 9, GEC_ScriptsCustom6 = 10, GEC_ScriptsCustom7 = 11, GEC_ScriptsCustom8 = 12, GEC_Tag = 2 }
enum EGlobalEventType { GET_Unknown = 0 }
enum EGwintAggressionMode { EGAM_Defensive = 0 }
enum EGwintDifficultyMode { EGDM_Easy = 0 }
enum EInputKey { IK_1 = 49, IK_3 = 51, IK_A = 65, IK_Alt = 18, IK_Backspace = 8, IK_C = 67, IK_CapsLock = 20, IK_Ctrl = 17, IK_D = 68, IK_Delete = 46, IK_Down = 40, IK_E = 69, IK_End = 35, IK_Enter = 13, IK_Escape = 27, IK_Execute = 43, IK_F = 70, IK_Home = 36, IK_Insert = 45, IK_LControl = 162, IK_LShift = 160, IK_Left = 37, IK_LeftMouse = 1, IK_MiddleMouse = 3, IK_Mouse4 = 193, IK_Mouse5 = 194, IK_Mouse6 = 195, IK_Mouse7 = 196, IK_Mouse8 = 197, IK_MouseWheelDown = 237, IK_MouseWheelUp = 236, IK_None = 0, IK_NumLock = 156, IK_NumMinus = 109, IK_NumPad0 = 96, IK_NumPad1 = 97, IK_NumPad2 = 98, IK_NumPad3 = 99, IK_NumPad4 = 100, IK_NumPad5 = 101, IK_NumPad6 = 102, IK_NumPad7 = 103, IK_NumPad8 = 104, IK_NumPad9 = 105, IK_NumPeriod = 110, IK_NumPlus = 107, IK_NumSlash = 111, IK_NumStar = 106, IK_PS4_OPTIONS = 255, IK_PS4_TOUCH_PRESS = 256, IK_Pad_A_CROSS = 136, IK_Pad_B_CIRCLE = 137, IK_Pad_Back_Select = 141, IK_Pad_DigitDown = 143, IK_Pad_DigitLeft = 144, IK_Pad_DigitRight = 145, IK_Pad_DigitUp = 142, IK_Pad_LeftAxisY = 153, IK_Pad_LeftShoulder = 148, IK_Pad_LeftThumb = 146, IK_Pad_LeftTrigger = 150, IK_Pad_RightShoulder = 149, IK_Pad_RightThumb = 147, IK_Pad_RightTrigger = 151, IK_Pad_Start = 140, IK_Pad_X_SQUARE = 138, IK_Pad_Y_TRIANGLE = 139, IK_PageDown = 34, IK_PageUp = 33, IK_Pause = 19, IK_Print = 42, IK_PrintScrn = 44, IK_RControl = 163, IK_RShift = 161, IK_Right = 39, IK_RightMouse = 2, IK_ScrollLock = 157, IK_Select = 41, IK_Separator = 108, IK_Shift = 16, IK_Space = 32, IK_Tab = 9, IK_Up = 38, IK_V = 86, IK_X = 88, IK_Z = 90 }
enum EInteractionPriority { IP_Max_Unpushable = -2, IP_NotSet = -1, IP_Prio_0 = 0, IP_Prio_12 = 12, IP_Prio_3 = 3, IP_Prio_5 = 5 }
enum EInventoryEventType { IET_ItemQuantityChanged = 3, IET_ItemRemoved = 2 }
enum EJournalStatus { JS_Active = 1, JS_Failed = 3, JS_Inactive = 0, JS_Success = 2 }
enum ELightShadowCastingMode { LSCM_None = 0, LSCM_Normal = 1, LSCM_OnlyDynamic = 2, LSCM_OnlyStatic = 3 }
enum ELoadGameResult { LOAD_Error = 4, LOAD_Initializing = 1, LOAD_Loading = 3, LOAD_MissingContent = 5, LOAD_NotInitialized = 0 }
enum EMinigameState { EMS_End_PlayerForfeited = 256, EMS_End_PlayerLost = 32, EMS_End_PlayerWon = 16, EMS_None = 2 }
enum EMountType { MT_instant = 2, MT_normal = 1 }
enum EMoveFailureAction { MFA_EXIT = 1 }
enum EMoveType { MT_AbsSpeed = 4, MT_FastRun = 2, MT_Run = 1, MT_Sprint = 3, MT_Walk = 0 }
enum EMovementAdjustmentNotify { MAN_LocationAdjustmentReachedDestination = 1 }
enum ENPCGroupType { ENGT_Commoner = 1, ENGT_Enemy = 0, ENGT_Guard = 3, ENGT_Quest = 2 }
enum ENavigationReachabilityTestType { ENavigationReachability_Any = 0 }
enum ENewGamePlusStatus { NGP_CantLoad = 2, NGP_ContentRequired = 6, NGP_InternalError = 5, NGP_Invalid = 1, NGP_RequirementsNotMet = 4, NGP_Success = 0, NGP_TooOld = 3 }
enum ENpcStance { NS_Fly = 5, NS_Guarded = 3, NS_Normal = 0, NS_Retreat = 2, NS_Strafe = 1, NS_Swim = 6, NS_Wounded = 4 }
enum EOrientationTarget { OT_Actor = 1, OT_Camera = 3, OT_CameraOffset = 4, OT_CustomHeading = 2, OT_None = 5, OT_Player = 0 }
enum EPersistanceMode { PM_DontPersist = 0, PM_Persist = 2 }
enum EPropertyAnimationOperation { PAO_Play = 0, PAO_Rewind = 2, PAO_Stop = 1 }
enum EPropertyCurveMode { PCM_Backward = 1, PCM_Forward = 0 }
enum EQuestManageFastTravelOperation { QMFT_EnableAndShow = 0, QMFT_EnableOnly = 1, QMFT_ShowOnly = 2 }
enum ER4CommonStats { CS_DIFFICULTY_LVL = 7, CS_TOXICITY = 1, CS_VITALITY = 0 }
enum ER4TelemetryEvents { TE_ELIXIR_USED = 30, TE_FIGHT_ENEMY_DIES = 25, TE_FIGHT_ENEMY_GETS_HIT = 26, TE_FIGHT_HERO_GETS_HIT = 27, TE_FIGHT_HERO_THROWS_BOMB = 28, TE_FIGHT_PLAYER_ATTACKS = 23, TE_FIGHT_PLAYER_DIES = 22, TE_FIGHT_PLAYER_USE_SIGN = 24, TE_HERO_CASH_CHANGED = 12, TE_HERO_EXP_EARNED = 9, TE_HERO_FOCUS_OFF = 15, TE_HERO_FOCUS_ON = 14, TE_HERO_GWENT_MATCH_ENDED = 19, TE_HERO_GWENT_MATCH_STARTED = 18, TE_HERO_LEVEL_UP = 8, TE_HERO_MUTAGEN_USED = 16, TE_HERO_SKILL_POINT_EARNED = 10, TE_HERO_SKILL_UP = 11, TE_INV_ITEM_BOUGHT = 36, TE_INV_ITEM_DROPPED = 34, TE_INV_ITEM_EQUIPPED = 31, TE_INV_ITEM_PICKED = 33, TE_INV_ITEM_SOLD = 35, TE_INV_ITEM_UNEQUIPPED = 32, TE_INV_QUEST_COMPLETED = 37, TE_ITEM_COOKED = 29, TE_STATE_AIM_THROW = 2, TE_STATE_COMBAT = 3, TE_STATE_DIALOG = 5, TE_STATE_EXPLORING = 4, TE_STATE_HORSE_RIDING = 0, TE_STATE_SAILING = 1, TE_STATE_SWIMMING = 6 }
enum ERidingManagerTask { RMT_DismountHorse = 2, RMT_None = 0 }
enum ESaveGameType { SGT_AutoSave = 1, SGT_CheckPoint = 5, SGT_ForcedCheckPoint = 4, SGT_Manual = 3, SGT_QuickSave = 2 }
enum ESessionRestoreResult { RESTORE_DLCRequired = 2, RESTORE_DataCorrupted = 1, RESTORE_InternalError = 4, RESTORE_MissingContent = 3, RESTORE_NoGameDefinition = 5, RESTORE_WrongGameVersion = 6 }
enum EShowFlags { SHOW_AI = 1, SHOW_Containers = 215, SHOW_Exploration = 28 }
enum ESpawnTreeSpawnVisibility { STSV_SPAWN_HIDEN = 0 }
enum EStorySceneSignalType { SSST_Accept = 0, SSST_Highlight = 1, SSST_Skip = 2 }
enum ESyncRotationUsingRefBoneType { SRT_TowardsOtherEntity = 0 }
enum ETickGroup { TICK_Main = 2, TICK_PrePhysics = 0 }
enum EUsableItemType { UI_Horn = 1, UI_Mask = 4, UI_Meteor = 6 }
enum EVehicleMountStatus { VMS_dismountInProgress = 2, VMS_dismounted = 3, VMS_mountInProgress = 0, VMS_mounted = 1 }
enum EVehicleMountType { VMT_ApproachAndMount = 1, VMT_ImmediateUse = 4, VMT_MountIfPossible = 2, VMT_TeleportAndMount = 3 }
enum EVehicleSlot { EVS_driver_slot = 0, EVS_passenger_slot = 1 }
enum EVehicleType { EVT_Boat = 1, EVT_Horse = 0, EVT_Undefined = 2 }
enum EWitcherSwordType { WST_Silver = 0, WST_Steel = 1 }
enum EWoundTypeFlags { WTF_All = 7, WTF_Cut = 1, WTF_Explosion = 2, WTF_Frost = 4 }
enum WLSP_TooHardBasket { BTAP_AboveEmergency2 = 66, DT_fromScript = 1024, DialogAction_CRAFTING = 1024, DialogAction_GAME_FIGHT = 256, DialogAction_GIFT = 262144, DialogAction_PERSUASION = 32, DialogAction_STORAGE = 131072, EQQF_IMPACT = 1, FLAG_Attitude_Friendly = 128, FLAG_Attitude_Hostile = 256, FLAG_Attitude_Neutral = 64, FLAG_ExcludePlayer = 1, FLAG_ExcludeTarget = 32, FLAG_OnlyActors = 2, FLAG_OnlyAliveActors = 4, FLAG_TestLineOfSight = 16384, GEC_Last = 14, GTFX_MultiFeedback = 3, GTFX_MultiVibration = 5, GTFX_Off = 0, GTFX_Vibration = 4, GTFX_Weapon = 6, IK_R = 82, IP_Prio_14 = 14, MT_fromScript = 1024, PM_Camera_FOV = 1, PM_Camera_Tilt = 2, PM_ChromaticAberration = 12, PM_Contrast = 8, PM_DOF_Aperture = 5, PM_DOF_Autofocus = 4, PM_DOF_Enable = 3, PM_DOF_FocusDistance = 6, PM_Exposure = 7, PM_Grain = 13, PM_Highlights = 9, PM_Saturation = 11, PM_Temperature = 10, PM_Vignette = 16, SCO_Local = 20, SCO_Uploading = 60, SO_Disable = 5, SO_Enable = 4, SO_Lock = 6, SO_Reset = 3, SO_Toggle = 2, SO_TurnOff = 1, SO_TurnOn = 0, SO_Unlock = 7 }
enum eGwintEffect { GwintEffect_11thCard = 8, GwintEffect_Bin2Pick1 = 17, GwintEffect_ClearSky = 30, GwintEffect_ClearWeather = 9, GwintEffect_CounterKingAbility = 22, GwintEffect_Draw2 = 34, GwintEffect_Horn = 27, GwintEffect_ImproveNeightbours = 32, GwintEffect_Melee = 23, GwintEffect_MeleeHorn = 18, GwintEffect_MeleeScorch = 7, GwintEffect_None = 0, GwintEffect_Nurse = 33, GwintEffect_PickFogCard = 12, GwintEffect_PickFrostCard = 13, GwintEffect_PickRainCard = 11, GwintEffect_PickWeatherCard = 10, GwintEffect_Ranged = 24, GwintEffect_RangedHorn = 19, GwintEffect_ResurectCard = 15, GwintEffect_ResurectFromEnemy = 16, GwintEffect_SameTypeMorale = 35, GwintEffect_Scorch = 29, GwintEffect_Siege = 25, GwintEffect_SiegeHorn = 20, GwintEffect_SiegeScorch = 21, GwintEffect_SummonClones = 31, GwintEffect_UnsummonDummy = 26, GwintEffect_View3EnemyCard = 14 }
enum eGwintFaction { GwintFaction_Neutral = 0, GwintFaction_Nilfgaard = 2, GwintFaction_NoMansLand = 1, GwintFaction_NothernKingdom = 3, GwintFaction_Scoiatael = 4, GwintFaction_Skellige = 5 }
enum eGwintType { GwintType_Creature = 8, GwintType_Hero = 128, GwintType_Melee = 1, GwintType_Ranged = 2, GwintType_Siege = 4, GwintType_Weather = 16 }
enum eQuestType { Chapter = 1, MonsterHunt = 3, Side = 2, Story = 0, TreasureHunt = 4 }
