enum EActorActionType {
    ActorAction_None = 0,
    ActorAction_Moving = 1,
    ActorAction_Rotating = 2,
    ActorAction_Animation = 4,
    ActorAction_RaiseEvent = 8,
    ActorAction_Sliding = 16,
    ActorAction_Working = 32,
    ActorAction_ChangeEmotion = 64,
    ActorAction_Exploration = 128,
    ActorAction_UseDevice = 256,
    ActorAction_DynamicMoving = 512,
    ActorAction_MovingOnCurve = 1024,
    ActorAction_CustomSteer = 2048,
    ActorAction_R4Reserved_PC = 4096,
    ActorAction_MovingOutNavdata = 8192
}

enum EAIAreaSelectionMode {
    EAIASM_Encounter = 0,
    EAIASM_GuardArea = 1,
    EAIASM_ByTag = 2,
    EAIASM_ByTagInEncounter = 3,
    EAIASM_None = 4
}

enum EAIAttitude {
    AIA_Friendly = 0,
    AIA_Neutral = 1,
    AIA_Hostile = 2
}

enum EAnimationEventType {
    AET_Tick = 0,
    AET_DurationStart = 1,
    AET_DurationStartInTheMiddle = 2,
    AET_DurationEnd = 3,
    AET_Duration = 4
}

enum EAnimationManualSyncType {
    AMST_SyncBeginning = 0,
    AMST_SyncEnd = 1,
    AMST_SyncMatchEvents = 2
}

enum EArbitratorPriorities {
    BTAP_Unavailable = -1,
    BTAP_BelowIdle = 16,
    BTAP_Idle = 20,
    BTAP_AboveIdle = 26,
    BTAP_AboveIdle2 = 31,
    BTAP_Emergency = 50,
    BTAP_AboveEmergency = 61,
    BTAP_AboveEmergency2 = 66,
    BTAP_Combat = 75,
    BTAP_AboveCombat = 86,
    BTAP_AboveCombat2 = 91,
    BTAP_FullCutscene = 95
}

enum EAsyncCheckResult {
    ASR_InProgress = 0,
    ASR_ReadyTrue = 1,
    ASR_ReadyFalse = 2,
    ASR_Failed = 3
}

enum EAsyncTestResult {
    EAsyncTastResult_Failure = 0,
    EAsyncTastResult_Success = 1,
    EAsyncTastResult_Pending = 2,
    EAsyncTastResult_Invalidated = 3
}

enum EAttackDirection {
    AD_Front = 0,
    AD_Left = 1,
    AD_Right = 2,
    AD_Back = 3
}

enum EAttackDistance {
    ADIST_Small = 0,
    ADIST_Medium = 1,
    ADIST_Large = 2,
    ADIST_None = 4
}

enum EBaseCharacterStats {
    BCS_Vitality = 0,
    BCS_Essence = 1,
    BCS_Stamina = 2,
    BCS_Toxicity = 3,
    BCS_Focus = 4,
    BCS_Morale = 5,
    BCS_Air = 6,
    BCS_Panic = 7,
    BCS_PanicStatic = 8,
    BCS_SwimmingStamina = 9,
    BCS_Undefined = 10
}

enum EBatchQueryState {
    BQS_NotFound = 0,
    BQS_NotReady = 1,
    BQS_Processed = 2
}

enum EBTNodeStatus {
    BTNS_Active = 0,
    BTNS_Failed = 1,
    BTNS_Completed = 2
}

enum EBufferActionType {
    EBAT_EMPTY = 0,
    EBAT_LightAttack = 1,
    EBAT_HeavyAttack = 2,
    EBAT_CastSign = 3,
    EBAT_ItemUse = 4,
    EBAT_Parry = 5,
    EBAT_Dodge = 6,
    EBAT_SpecialAttack_Light = 7,
    EBAT_SpecialAttack_Heavy = 8,
    EBAT_Roll = 9,
    EBAT_Ciri_SpecialAttack = 10,
    EBAT_Ciri_SpecialAttack_Heavy = 11,
    EBAT_Ciri_Counter = 12,
    EBAT_Ciri_Dodge = 13,
    EBAT_Draw_Steel = 14,
    EBAT_Draw_Silver = 15,
    EBAT_Sheathe_Sword = 16
}

enum ECharacterDefenseStats {
    CDS_None = 0,
    CDS_PhysicalRes = 1,
    CDS_BleedingRes = 2,
    CDS_PoisonRes = 3,
    CDS_FireRes = 4,
    CDS_FrostRes = 5,
    CDS_ShockRes = 6,
    CDS_ForceRes = 7,
    CDS_FreezeRes = 8,
    CDS_WillRes = 9,
    CDS_BurningRes = 10,
    CDS_SlashingRes = 11,
    CDS_PiercingRes = 12,
    CDS_BludgeoningRes = 13,
    CDS_RendingRes = 14,
    CDS_ElementalRes = 15,
    CDS_DoTBurningDamageRes = 16,
    CDS_DoTPoisonDamageRes = 17,
    CDS_DoTBleedingDamageRes = 18
}

enum ECharacterPhysicsState {
    CPS_Simulated = 0,
    CPS_Animated = 1,
    CPS_Falling = 2,
    CPS_Swimming = 3,
    CPS_Ragdoll = 4,
    CPS_Count = 5
}

enum ECollisionSides {
    CS_FRONT = 0,
    CS_RIGHT = 1,
    CS_BACK = 2,
    CS_LEFT = 3,
    CS_FRONT_LEFT = 4,
    CS_FRONT_RIGHT = 5,
    CS_BACK_RIGHT = 6,
    CS_BACK_LEFT = 7,
    CS_CENTER = 8
}

enum ECombatActionType {
    CAT_Attack = 0,
    CAT_SpecialAttack = 1,
    CAT_Dodge = 2,
    CAT_Roll = 3,
    CAT_ItemThrow = 4,
    CAT_LayItem = 5,
    CAT_CastSign = 6,
    CAT_Pirouette = 7,
    CAT_PreAttack = 8,
    CAT_Parry = 9,
    CAT_Crossbow = 10,
    CAT_None2 = 11,
    CAT_CiriDodge = 12
}

enum ECombatTargetSelectionSkipTarget {
    CTSST_SKIP_ALWAYS = 0,
    CTSST_SKIP_IF_THERE_ARE_OTHER_TARGETS = 1,
    CTSST_DONT_SKIP = 2
}

enum EComboAttackType {
    ComboAT_Normal = 0,
    ComboAT_Directional = 1,
    ComboAT_Restart = 2,
    ComboAT_Stop = 3
}

enum EDialogActionIcon {
    DialogAction_LEVELUP3 = -2147483648,
    DialogAction_NONE = 1,
    DialogAction_AXII = 2,
    DialogAction_CONTENT_MISSING = 4,
    DialogAction_BRIBE = 8,
    DialogAction_HOUSE = 16,
    DialogAction_PERSUASION = 32,
    DialogAction_GETBACK = 64,
    DialogAction_GAME_DICES = 128,
    DialogAction_GAME_FIGHT = 256,
    DialogAction_GAME_WRESTLE = 512,
    DialogAction_CRAFTING = 1024,
    DialogAction_SHOPPING = 2048,
    DialogAction_TimedChoice = 4096,
    DialogAction_EXIT = 8192,
    DialogAction_HAIRCUT = 16384,
    DialogAction_MONSTERCONTRACT = 32768,
    DialogAction_BET = 65536,
    DialogAction_STORAGE = 131072,
    DialogAction_GIFT = 262144,
    DialogAction_GAME_DRINK = 524288,
    DialogAction_GAME_DAGGER = 1048576,
    DialogAction_SMITH = 2097152,
    DialogAction_ARMORER = 4194304,
    DialogAction_RUNESMITH = 8388608,
    DialogAction_TEACHER = 16777216,
    DialogAction_FAST_TRAVEL = 33554432,
    DialogAction_GAME_CARDS = 67108864,
    DialogAction_SHAVING = 134217728,
    DialogAction_AUCTION = 268435456,
    DialogAction_LEVELUP1 = 536870912,
    DialogAction_LEVELUP2 = 1073741824
}

enum EDifficultyMode {
    EDM_NotSet = 0,
    EDM_Easy = 1,
    EDM_Medium = 2,
    EDM_Hard = 3,
    EDM_Hardcore = 4
}

enum EDismountType {
    DT_normal = 1,
    DT_shakeOff = 2,
    DT_ragdoll = 4,
    DT_instant = 8,
    DT_fromScript = 1024
}

enum EEntityGameplayEffectFlags {
    EGEF_FocusModeHighlight = 1,
    EGEF_CatViewHiglight = 2
}

enum EExplorationType {
    ET_Jump = 0,
    ET_Ladder = 1,
    ET_Horse_LF = 2,
    ET_Horse_LB = 3,
    ET_Horse_L = 4,
    ET_Horse_R = 5,
    ET_Horse_RF = 6,
    ET_Horse_RB = 7,
    ET_Horse_B = 8,
    ET_Boat_B = 9,
    ET_Boat_P = 10,
    ET_Boat_Enter_From_Beach = 11,
    ET_Fence = 12,
    ET_Fence_OneSided = 13,
    ET_Ledge = 14,
    ET_Boat_Passenger_B = 15
}

enum EFinisherSide {
    FinisherLeft = 0,
    FinisherRight = 1
}

enum EFocusModeVisibility {
    FMV_None = 0,
    FMV_Interactive = 1,
    FMV_Clue = 2
}

enum EGlobalEventCategory {
    GEC_Empty = 0,
    GEC_Trigger = 1,
    GEC_Tag = 2,
    GEC_Fact = 3,
    GEC_ScriptsCustom0 = 4,
    GEC_ScriptsCustom1 = 5,
    GEC_ScriptsCustom2 = 6,
    GEC_ScriptsCustom3 = 7,
    GEC_ScriptsCustom4 = 8,
    GEC_ScriptsCustom5 = 9,
    GEC_ScriptsCustom6 = 10,
    GEC_ScriptsCustom7 = 11,
    GEC_ScriptsCustom8 = 12,
    GEC_ScriptsCustom9 = 13,
    GEC_Last = 14
}

enum EGlobalEventType {
    GET_Unknown = 0,
    GET_TriggerCreated = 1,
    GET_TriggerRemoved = 2,
    GET_TriggerActivatorCreated = 3,
    GET_TriggerActivatorRemoved = 4,
    GET_TagAdded = 5,
    GET_TagRemoved = 6,
    GET_StubTagAdded = 7,
    GET_StubTagRemoved = 8,
    GET_FactAdded = 9,
    GET_FactRemoved = 10,
    GET_ScriptsCustom0 = 11,
    GET_ScriptsCustom1 = 12,
    GET_ScriptsCustom2 = 13,
    GET_ScriptsCustom3 = 14
}

enum EGwintAggressionMode {
    EGAM_Defensive = 0,
    EGAM_Normal = 1,
    EGAM_Aggressive = 2,
    EGAM_VeryAggressive = 3,
    EGAM_AllIHave = 4
}

enum EGwintDifficultyMode {
    EGDM_Easy = 0,
    EGDM_Medium = 1,
    EGDM_Hard = 2
}

enum EInteractionPriority {
    IP_Max_Unpushable = -2,
    IP_NotSet = -1,
    IP_Prio_0 = 0,
    IP_Prio_1 = 1,
    IP_Prio_2 = 2,
    IP_Prio_3 = 3,
    IP_Prio_4 = 4,
    IP_Prio_5 = 5,
    IP_Prio_6 = 6,
    IP_Prio_7 = 7,
    IP_Prio_8 = 8,
    IP_Prio_9 = 9,
    IP_Prio_10 = 10,
    IP_Prio_11 = 11,
    IP_Prio_12 = 12,
    IP_Prio_13 = 13,
    IP_Prio_14 = 14
}

enum EInventoryEventType {
    IET_Empty = 0,
    IET_ItemAdded = 1,
    IET_ItemRemoved = 2,
    IET_ItemQuantityChanged = 3,
    IET_ItemTagChanged = 4,
    IET_InventoryRebalanced = 5
}

enum EJournalStatus {
    JS_Inactive = 0,
    JS_Active = 1,
    JS_Success = 2,
    JS_Failed = 3
}

enum ELightShadowCastingMode {
    LSCM_None = 0,
    LSCM_Normal = 1,
    LSCM_OnlyDynamic = 2,
    LSCM_OnlyStatic = 3
}

enum ELoadGameResult {
    LOAD_NotInitialized = 0,
    LOAD_Initializing = 1,
    LOAD_ReadyToLoad = 2,
    LOAD_Loading = 3,
    LOAD_Error = 4,
    LOAD_MissingContent = 5
}

enum EMinigameState {
    EMS_None = 2,
    EMS_Init = 4,
    EMS_Started = 8,
    EMS_End_PlayerWon = 16,
    EMS_End_PlayerLost = 32,
    EMS_End_Error = 64,
    EMS_Wait_PlayerLost = 128,
    EMS_End_PlayerForfeited = 256,
    EMS_End = 368
}

enum EMountType {
    MT_normal = 1,
    MT_instant = 2,
    MT_fromScript = 1024
}

enum EMoveFailureAction {
    MFA_REPLAN = 0,
    MFA_EXIT = 1
}

enum EMovementAdjustmentNotify {
    MAN_None = 0,
    MAN_LocationAdjustmentReachedDestination = 1,
    MAN_RotationAdjustmentReachedDestination = 2,
    MAN_AdjustmentEnded = 3,
    MAN_AdjustmentCancelled = 4
}

enum EMoveType {
    MT_Walk = 0,
    MT_Run = 1,
    MT_FastRun = 2,
    MT_Sprint = 3,
    MT_AbsSpeed = 4
}

enum ENavigationReachabilityTestType {
    ENavigationReachability_Any = 0,
    ENavigationReachability_All = 1,
    ENavigationReachability_FullTest = 2
}

enum ENewGamePlusStatus {
    NGP_Success = 0,
    NGP_Invalid = 1,
    NGP_CantLoad = 2,
    NGP_TooOld = 3,
    NGP_RequirementsNotMet = 4,
    NGP_InternalError = 5,
    NGP_ContentRequired = 6,
    NGP_WrongGameVersion = 7
}

enum ENPCGroupType {
    ENGT_Enemy = 0,
    ENGT_Commoner = 1,
    ENGT_Quest = 2,
    ENGT_Guard = 3
}

enum ENpcStance {
    NS_Normal = 0,
    NS_Strafe = 1,
    NS_Retreat = 2,
    NS_Guarded = 3,
    NS_Wounded = 4,
    NS_Fly = 5,
    NS_Swim = 6
}

enum EOrientationTarget {
    OT_Player = 0,
    OT_Actor = 1,
    OT_CustomHeading = 2,
    OT_Camera = 3,
    OT_CameraOffset = 4,
    OT_None = 5
}

enum EPersistanceMode {
    PM_DontPersist = 0,
    PM_SaveStateOnly = 1,
    PM_Persist = 2
}

enum EPropertyAnimationOperation {
    PAO_Play = 0,
    PAO_Stop = 1,
    PAO_Rewind = 2,
    PAO_Pause = 3,
    PAO_Unpause = 4
}

enum EPropertyCurveMode {
    PCM_Forward = 0,
    PCM_Backward = 1
}

enum EQuestManageFastTravelOperation {
    QMFT_EnableAndShow = 0,
    QMFT_EnableOnly = 1,
    QMFT_ShowOnly = 2
}

enum ER4CommonStats {
    CS_VITALITY = 0,
    CS_TOXICITY = 1,
    CS_VIGOR = 2,
    CS_SKILLPOINTS = 3,
    CS_POSITION_X = 4,
    CS_POSITION_Y = 5,
    CS_POSITION_Z = 6,
    CS_DIFFICULTY_LVL = 7,
    CS_GAME_PROGRESS = 8,
    CS_MEMORY = 9,
    CS_FPS = 10,
    CS_WORLD_ID = 11,
    CS_GAME_TIME = 12,
    CS_UNKNOWN = 13
}

enum ER4TelemetryEvents {
    TE_STATE_HORSE_RIDING = 0,
    TE_STATE_SAILING = 1,
    TE_STATE_AIM_THROW = 2,
    TE_STATE_COMBAT = 3,
    TE_STATE_EXPLORING = 4,
    TE_STATE_DIALOG = 5,
    TE_STATE_SWIMMING = 6,
    TE_HERO_FAST_TRAVEL = 7,
    TE_HERO_LEVEL_UP = 8,
    TE_HERO_EXP_EARNED = 9,
    TE_HERO_SKILL_POINT_EARNED = 10,
    TE_HERO_SKILL_UP = 11,
    TE_HERO_CASH_CHANGED = 12,
    TE_HERO_SPAWNED = 13,
    TE_HERO_FOCUS_ON = 14,
    TE_HERO_FOCUS_OFF = 15,
    TE_HERO_MUTAGEN_USED = 16,
    TE_HERO_ACHIEVEMENT_UNLOCKED = 17,
    TE_HERO_GWENT_MATCH_STARTED = 18,
    TE_HERO_GWENT_MATCH_ENDED = 19,
    TE_HERO_HEALTH_SEGMENT_LOST = 20,
    TE_HERO_HEALTH_SEGMENT_REGAINED = 21,
    TE_FIGHT_PLAYER_DIES = 22,
    TE_FIGHT_PLAYER_ATTACKS = 23,
    TE_FIGHT_PLAYER_USE_SIGN = 24,
    TE_FIGHT_ENEMY_DIES = 25,
    TE_FIGHT_ENEMY_GETS_HIT = 26,
    TE_FIGHT_HERO_GETS_HIT = 27,
    TE_FIGHT_HERO_THROWS_BOMB = 28,
    TE_ITEM_COOKED = 29,
    TE_ELIXIR_USED = 30,
    TE_INV_ITEM_EQUIPPED = 31,
    TE_INV_ITEM_UNEQUIPPED = 32,
    TE_INV_ITEM_PICKED = 33,
    TE_INV_ITEM_DROPPED = 34,
    TE_INV_ITEM_SOLD = 35,
    TE_INV_ITEM_BOUGHT = 36,
    TE_INV_QUEST_COMPLETED = 37,
    TE_HERO_MOVEMENT = 38,
    TE_HERO_POSITION = 39,
    TE_SYS_END_SESISON = 40,
    TE_SYS_GAME_LOADED = 41,
    TE_SYS_GAME_SAVED = 42,
    TE_SYS_GAME_LAUNCHED = 43,
    TE_SYS_GAME_PAUSE = 44,
    TE_SYS_GAME_UNPAUSE = 45,
    TE_SYS_GAME_PROGRESS = 46,
    TE_QUEST_ACTIVATED = 47,
    TE_QUEST_FINISHED = 48,
    TE_QUEST_FAILED = 49,
    TE_UNKNOWN = 50
}

enum ERidingManagerTask {
    RMT_None = 0,
    RMT_MountHorse = 1,
    RMT_DismountHorse = 2,
    RMT_MountBoat = 3,
    RMT_DismountBoat = 4
}

enum ESaveGameType {
    SGT_AutoSave = 1,
    SGT_QuickSave = 2,
    SGT_Manual = 3,
    SGT_ForcedCheckPoint = 4,
    SGT_CheckPoint = 5
}

enum ESessionRestoreResult {
    RESTORE_Success = 0,
    RESTORE_DataCorrupted = 1,
    RESTORE_DLCRequired = 2,
    RESTORE_MissingContent = 3,
    RESTORE_InternalError = 4,
    RESTORE_NoGameDefinition = 5,
    RESTORE_WrongGameVersion = 6
}

enum ESpawnTreeSpawnVisibility {
    STSV_SPAWN_HIDEN = 0,
    STSV_SPAWN_ALWAYS = 1,
    STSV_SPAWN_ONLY_VISIBLE = 2
}

enum EStorySceneSignalType {
    SSST_Accept = 0,
    SSST_Highlight = 1,
    SSST_Skip = 2
}

enum ESyncRotationUsingRefBoneType {
    SRT_TowardsOtherEntity = 0,
    SRT_ToMatchOthersRotation = 1
}

enum ETickGroup {
    TICK_PrePhysics = 0,
    TICK_PrePhysicsPost = 1,
    TICK_Main = 2,
    TICK_PostPhysics = 3,
    TICK_PostPhysicsPost = 4,
    TICK_PostUpdateTransform = 5
}

enum EUsableItemType {
    UI_Torch = 0,
    UI_Horn = 1,
    UI_Bell = 2,
    UI_OilLamp = 3,
    UI_Mask = 4,
    UI_FiendLure = 5,
    UI_Meteor = 6,
    UI_None = 7,
    UI_Censer = 8,
    UI_Apple = 9,
    UI_Cookie = 10,
    UI_Basket = 11
}

enum EVehicleMountStatus {
    VMS_mountInProgress = 0,
    VMS_mounted = 1,
    VMS_dismountInProgress = 2,
    VMS_dismounted = 3
}

enum EVehicleMountType {
    VMT_None = 0,
    VMT_ApproachAndMount = 1,
    VMT_MountIfPossible = 2,
    VMT_TeleportAndMount = 3,
    VMT_ImmediateUse = 4
}

enum EVehicleSlot {
    EVS_driver_slot = 0,
    EVS_passenger_slot = 1
}

enum EVehicleType {
    EVT_Horse = 0,
    EVT_Boat = 1,
    EVT_Undefined = 2
}

enum EWitcherSwordType {
    WST_Silver = 0,
    WST_Steel = 1
}

enum EWoundTypeFlags {
    WTF_None = 0,
    WTF_Cut = 1,
    WTF_Explosion = 2,
    WTF_Frost = 4,
    WTF_All = 7
}

enum eGwintEffect {
    GwintEffect_None = 0,
    GwintEffect_Bin2 = 5,
    GwintEffect_MeleeScorch = 7,
    GwintEffect_11thCard = 8,
    GwintEffect_ClearWeather = 9,
    GwintEffect_PickWeatherCard = 10,
    GwintEffect_PickRainCard = 11,
    GwintEffect_PickFogCard = 12,
    GwintEffect_PickFrostCard = 13,
    GwintEffect_View3EnemyCard = 14,
    GwintEffect_ResurectCard = 15,
    GwintEffect_ResurectFromEnemy = 16,
    GwintEffect_Bin2Pick1 = 17,
    GwintEffect_MeleeHorn = 18,
    GwintEffect_RangedHorn = 19,
    GwintEffect_SiegeHorn = 20,
    GwintEffect_SiegeScorch = 21,
    GwintEffect_CounterKingAbility = 22,
    GwintEffect_Melee = 23,
    GwintEffect_Ranged = 24,
    GwintEffect_Siege = 25,
    GwintEffect_UnsummonDummy = 26,
    GwintEffect_Horn = 27,
    GwintEffect_Draw = 28,
    GwintEffect_Scorch = 29,
    GwintEffect_ClearSky = 30,
    GwintEffect_SummonClones = 31,
    GwintEffect_ImproveNeightbours = 32,
    GwintEffect_Nurse = 33,
    GwintEffect_Draw2 = 34,
    GwintEffect_SameTypeMorale = 35,
    GwintEffect_Mushroom = 41,
    GwintEffect_Morph = 42,
    GwintEffect_WeatherResistant = 43,
    GwintEffect_GraveyardShuffle = 44
}

enum eGwintFaction {
    GwintFaction_Neutral = 0,
    GwintFaction_NoMansLand = 1,
    GwintFaction_Nilfgaard = 2,
    GwintFaction_NothernKingdom = 3,
    GwintFaction_Scoiatael = 4,
    GwintFaction_Skellige = 5
}

enum eGwintType {
    GwintType_None = 0,
    GwintType_Melee = 1,
    GwintType_Ranged = 2,
    GwintType_Siege = 4,
    GwintType_Creature = 8,
    GwintType_Weather = 16,
    GwintType_Spell = 32,
    GwintType_RowModifier = 64,
    GwintType_Hero = 128,
    GwintType_Spy = 256,
    GwintType_FriendlyEffect = 512,
    GwintType_OffensiveEffect = 1024,
    GwintType_GlobalEffect = 2048
}

enum eQuestType {
    Story = 0,
    Chapter = 1,
    Side = 2,
    MonsterHunt = 3,
    TreasureHunt = 4
}
