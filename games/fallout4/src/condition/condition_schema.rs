//! Generated from the pinned Fallout 4 CTDA function schema.
//! Source revision: 4f533562ee0c70347d47c1979d5464d42b06ee6b; Condition.cs SHA-256: 1d7658bc37b09a8cf8366d61650d13b712100d223e4aced134c1f04fcd3bb3d2.
//! Regenerate with py -3.13 tools/generate-condition-schema.py, then format with tools/cargo.ps1.
use crate::condition::FunctionParameterHint;

const FUNCTION_NAMES: &[(u16, &str)] = &[
    (0, "GetWantBlocking"),
    (1, "GetDistance"),
    (5, "GetLocked"),
    (6, "GetPos"),
    (8, "GetAngle"),
    (10, "GetStartingPos"),
    (11, "GetStartingAngle"),
    (12, "GetSecondsPassed"),
    (14, "GetValue"),
    (18, "GetCurrentTime"),
    (24, "GetScale"),
    (25, "IsMoving"),
    (26, "IsTurning"),
    (27, "GetLineOfSight"),
    (32, "GetInSameCell"),
    (35, "GetDisabled"),
    (36, "MenuMode"),
    (39, "GetDisease"),
    (41, "GetClothingValue"),
    (42, "SameFaction"),
    (43, "SameRace"),
    (44, "SameSex"),
    (45, "GetDetected"),
    (46, "GetDead"),
    (47, "GetItemCount"),
    (48, "GetGold"),
    (49, "GetSleeping"),
    (50, "GetTalkedToPC"),
    (56, "GetQuestRunning"),
    (58, "GetStage"),
    (59, "GetStageDone"),
    (60, "GetFactionRankDifference"),
    (61, "GetAlarmed"),
    (62, "IsRaining"),
    (63, "GetAttacked"),
    (64, "GetIsCreature"),
    (65, "GetLockLevel"),
    (66, "GetShouldAttack"),
    (67, "GetInCell"),
    (68, "GetIsClass"),
    (69, "GetIsRace"),
    (70, "GetIsSex"),
    (71, "GetInFaction"),
    (72, "GetIsID"),
    (73, "GetFactionRank"),
    (74, "GetGlobalValue"),
    (75, "IsSnowing"),
    (77, "GetRandomPercent"),
    (79, "WouldBeStealing"),
    (80, "GetLevel"),
    (81, "IsRotating"),
    (84, "GetDeadCount"),
    (91, "GetIsAlerted"),
    (98, "GetPlayerControlsDisabled"),
    (99, "GetHeadingAngle"),
    (101, "IsWeaponMagicOut"),
    (102, "IsTorchOut"),
    (103, "IsShieldOut"),
    (106, "IsFacingUp"),
    (107, "GetKnockedState"),
    (108, "GetWeaponAnimType"),
    (109, "IsWeaponSkillType"),
    (110, "GetCurrentAIPackage"),
    (111, "IsWaiting"),
    (112, "IsIdlePlaying"),
    (116, "IsIntimidatedbyPlayer"),
    (117, "IsPlayerInRegion"),
    (118, "GetActorAggroRadiusViolated"),
    (122, "GetCrime"),
    (123, "IsGreetingPlayer"),
    (125, "IsGuard"),
    (127, "HasBeenEaten"),
    (128, "GetStaminaPercentage"),
    (129, "HasBeenRead"),
    (130, "GetDying"),
    (131, "GetSceneActionPercent"),
    (132, "WouldRefuseCommand"),
    (133, "SameFactionAsPC"),
    (134, "SameRaceAsPC"),
    (135, "SameSexAsPC"),
    (136, "GetIsReference"),
    (141, "IsTalking"),
    (142, "GetComponentCount"),
    (143, "GetCurrentAIProcedure"),
    (144, "GetTrespassWarningLevel"),
    (145, "IsTrespassing"),
    (146, "IsInMyOwnedCell"),
    (147, "GetWindSpeed"),
    (148, "GetCurrentWeatherPercent"),
    (149, "GetIsCurrentWeather"),
    (150, "IsContinuingPackagePCNear"),
    (152, "GetIsCrimeFaction"),
    (153, "CanHaveFlames"),
    (154, "HasFlames"),
    (157, "GetOpenState"),
    (159, "GetSitting"),
    (161, "GetIsCurrentPackage"),
    (162, "IsCurrentFurnitureRef"),
    (163, "IsCurrentFurnitureObj"),
    (170, "GetDayOfWeek"),
    (172, "GetTalkedToPCParam"),
    (175, "IsPCSleeping"),
    (176, "IsPCAMurderer"),
    (180, "HasSameEditorLocationAsRef"),
    (181, "HasSameEditorLocationAsRefAlias"),
    (182, "GetEquipped"),
    (185, "IsSwimming"),
    (190, "GetAmountSoldStolen"),
    (192, "GetIgnoreCrime"),
    (193, "GetPCExpelled"),
    (195, "GetPCFactionMurder"),
    (197, "GetPCEnemyofFaction"),
    (199, "GetPCFactionAttack"),
    (203, "GetDestroyed"),
    (214, "HasMagicEffect"),
    (215, "GetDefaultOpen"),
    (223, "IsSpellTarget"),
    (224, "GetVATSMode"),
    (225, "GetPersuasionNumber"),
    (226, "GetVampireFeed"),
    (227, "GetCannibal"),
    (228, "GetIsClassDefault"),
    (229, "GetClassDefaultMatch"),
    (230, "GetInCellParam"),
    (231, "GetPlayerDialogueInput"),
    (235, "GetVatsTargetHeight"),
    (237, "GetIsGhost"),
    (242, "GetUnconscious"),
    (244, "GetRestrained"),
    (246, "GetIsUsedItem"),
    (247, "GetIsUsedItemType"),
    (248, "IsScenePlaying"),
    (249, "IsInDialogueWithPlayer"),
    (250, "GetLocationCleared"),
    (254, "GetIsPlayableRace"),
    (255, "GetOffersServicesNow"),
    (258, "HasAssociationType"),
    (259, "HasFamilyRelationship"),
    (261, "HasParentRelationship"),
    (262, "IsWarningAbout"),
    (263, "IsWeaponOut"),
    (264, "HasSpell"),
    (265, "IsTimePassing"),
    (266, "IsPleasant"),
    (267, "IsCloudy"),
    (274, "IsSmallBump"),
    (277, "GetBaseValue"),
    (278, "IsOwner"),
    (280, "IsCellOwner"),
    (282, "IsHorseStolen"),
    (285, "IsLeftUp"),
    (286, "IsSneaking"),
    (287, "IsRunning"),
    (288, "GetFriendHit"),
    (289, "IsInCombat"),
    (300, "IsInInterior"),
    (304, "IsWaterObject"),
    (305, "GetPlayerAction"),
    (306, "IsActorUsingATorch"),
    (309, "IsXBox"),
    (310, "GetInWorldspace"),
    (312, "GetPCMiscStat"),
    (313, "GetPairedAnimation"),
    (314, "IsActorAVictim"),
    (315, "GetTotalPersuasionNumber"),
    (318, "GetIdleDoneOnce"),
    (320, "GetNoRumors"),
    (323, "GetCombatState"),
    (325, "GetWithinPackageLocation"),
    (327, "IsRidingMount"),
    (329, "IsFleeing"),
    (332, "IsInDangerousWater"),
    (338, "GetIgnoreFriendlyHits"),
    (339, "IsPlayersLastRiddenMount"),
    (353, "IsActor"),
    (354, "IsEssential"),
    (358, "IsPlayerMovingIntoNewSpace"),
    (359, "GetInCurrentLocation"),
    (360, "GetInCurrentLocationAlias"),
    (361, "GetTimeDead"),
    (362, "HasLinkedRef"),
    (365, "IsChild"),
    (366, "GetStolenItemValueNoCrime"),
    (367, "GetLastPlayerAction"),
    (368, "IsPlayerActionActive"),
    (370, "IsTalkingActivatorActor"),
    (372, "IsInList"),
    (373, "GetStolenItemValue"),
    (375, "GetCrimeGoldViolent"),
    (376, "GetCrimeGoldNonviolent"),
    (378, "IsOwnedBy"),
    (380, "GetCommandDistance"),
    (381, "GetCommandLocationDistance"),
    (390, "GetHitLocation"),
    (391, "IsPC1stPerson"),
    (396, "GetCauseofDeath"),
    (397, "IsLimbGone"),
    (398, "IsWeaponInList"),
    (402, "IsBribedbyPlayer"),
    (403, "GetRelationshipRank"),
    (407, "GetVATSValue"),
    (408, "IsKiller"),
    (409, "IsKillerObject"),
    (410, "GetFactionCombatReaction"),
    (414, "Exists"),
    (415, "GetGroupMemberCount"),
    (416, "GetGroupTargetCount"),
    (426, "GetIsVoiceType"),
    (427, "GetPlantedExplosive"),
    (429, "IsScenePackageRunning"),
    (430, "GetHealthPercentage"),
    (432, "GetIsObjectType"),
    (434, "PlayerVisualDetection"),
    (435, "PlayerAudioDetection"),
    (437, "GetIsCreatureType"),
    (438, "HasKey"),
    (439, "IsFurnitureEntryType"),
    (444, "GetInCurrentLocationFormList"),
    (445, "GetInZone"),
    (446, "GetVelocity"),
    (447, "GetGraphVariableFloat"),
    (448, "HasPerk"),
    (449, "GetFactionRelation"),
    (450, "IsLastIdlePlayed"),
    (453, "GetPlayerTeammate"),
    (454, "GetPlayerTeammateCount"),
    (458, "GetActorCrimePlayerEnemy"),
    (459, "GetCrimeGold"),
    (463, "IsPlayerGrabbedRef"),
    (465, "GetKeywordItemCount"),
    (470, "GetDestructionStage"),
    (473, "GetIsAlignment"),
    (476, "IsProtected"),
    (477, "GetThreatRatio"),
    (479, "GetIsUsedItemEquipType"),
    (483, "GetPlayerActivated"),
    (485, "GetFullyEnabledActorsInHigh"),
    (487, "IsCarryable"),
    (488, "GetConcussed"),
    (491, "GetMapMarkerVisible"),
    (493, "PlayerKnows"),
    (494, "GetPermanentValue"),
    (495, "GetKillingBlowLimb"),
    (497, "CanPayCrimeGold"),
    (499, "GetDaysInJail"),
    (500, "EPAlchemyGetMakingPoison"),
    (501, "EPAlchemyEffectHasKeyword"),
    (503, "GetAllowWorldInteractions"),
    (506, "DialogueGetAv"),
    (507, "DialogueHasPerk"),
    (508, "GetLastHitCritical"),
    (510, "DialogueGetItemCount"),
    (511, "LastCrippledCondition"),
    (512, "HasSharedPowerGrid"),
    (513, "IsCombatTarget"),
    (515, "GetVATSRightAreaFree"),
    (516, "GetVATSLeftAreaFree"),
    (517, "GetVATSBackAreaFree"),
    (518, "GetVATSFrontAreaFree"),
    (519, "GetIsLockBroken"),
    (520, "IsPS3"),
    (521, "IsWindowsPC"),
    (522, "GetVATSRightTargetVisible"),
    (523, "GetVATSLeftTargetVisible"),
    (524, "GetVATSBackTargetVisible"),
    (525, "GetVATSFrontTargetVisible"),
    (528, "IsInCriticalStage"),
    (530, "GetXPForNextLevel"),
    (533, "GetInfamy"),
    (534, "GetInfamyViolent"),
    (535, "GetInfamyNonViolent"),
    (536, "GetTypeCommandPerforming"),
    (543, "GetQuestCompleted"),
    (544, "GetSpeechChallengeSuccessLevel"),
    (547, "IsGoreDisabled"),
    (550, "IsSceneActionComplete"),
    (552, "GetSpellUsageNum"),
    (554, "GetActorsInHigh"),
    (555, "HasLoaded3D"),
    (560, "HasKeyword"),
    (561, "HasRefType"),
    (562, "LocationHasKeyword"),
    (563, "LocationHasRefType"),
    (565, "GetIsEditorLocation"),
    (566, "GetIsAliasRef"),
    (567, "GetIsEditorLocationAlias"),
    (568, "IsSprinting"),
    (569, "IsBlocking"),
    (570, "HasEquippedSpell"),
    (571, "GetCurrentCastingType"),
    (572, "GetCurrentDeliveryType"),
    (574, "GetAttackState"),
    (576, "GetEventData"),
    (577, "IsCloserToAThanB"),
    (578, "LevelMinusPCLevel"),
    (580, "IsBleedingOut"),
    (584, "GetRelativeAngle"),
    (589, "GetMovementDirection"),
    (590, "IsInScene"),
    (591, "GetRefTypeDeadCount"),
    (592, "GetRefTypeAliveCount"),
    (594, "GetIsFlying"),
    (595, "IsCurrentSpell"),
    (596, "SpellHasKeyword"),
    (597, "GetEquippedItemType"),
    (598, "GetLocationAliasCleared"),
    (600, "GetLocationAliasRefTypeDeadCount"),
    (601, "GetLocationAliasRefTypeAliveCount"),
    (602, "IsWardState"),
    (603, "IsInSameCurrentLocationAsRef"),
    (604, "IsInSameCurrentLocationAsRefAlias"),
    (605, "LocationAliasIsLocation"),
    (606, "GetKeywordDataForLocation"),
    (608, "GetKeywordDataForAlias"),
    (610, "LocationAliasHasKeyword"),
    (611, "IsNullPackageData"),
    (612, "GetNumericPackageData"),
    (613, "IsPlayerRadioOn"),
    (614, "GetPlayerRadioFrequency"),
    (615, "GetHighestRelationshipRank"),
    (616, "GetLowestRelationshipRank"),
    (617, "HasAssociationTypeAny"),
    (618, "HasFamilyRelationshipAny"),
    (619, "GetPathingTargetOffset"),
    (620, "GetPathingTargetAngleOffset"),
    (621, "GetPathingTargetSpeed"),
    (622, "GetPathingTargetSpeedAngle"),
    (623, "GetMovementSpeed"),
    (624, "GetInContainer"),
    (625, "IsLocationLoaded"),
    (626, "IsLocationAliasLoaded"),
    (627, "IsDualCasting"),
    (629, "GetVMQuestVariable"),
    (630, "GetCombatAudioDetection"),
    (631, "GetCombatVisualDetection"),
    (632, "IsCasting"),
    (633, "GetFlyingState"),
    (635, "IsInFavorState"),
    (636, "HasTwoHandedWeaponEquipped"),
    (637, "IsFurnitureExitType"),
    (638, "IsInFriendStatewithPlayer"),
    (639, "GetWithinDistance"),
    (640, "GetValuePercent"),
    (641, "IsUnique"),
    (642, "GetLastBumpDirection"),
    (644, "GetInfoChallangeSuccess"),
    (645, "GetIsInjured"),
    (646, "GetIsCrashLandRequest"),
    (647, "GetIsHastyLandRequest"),
    (650, "IsLinkedTo"),
    (651, "GetKeywordDataForCurrentLocation"),
    (652, "GetInSharedCrimeFaction"),
    (654, "GetBribeSuccess"),
    (655, "GetIntimidateSuccess"),
    (656, "GetArrestedState"),
    (657, "GetArrestingActor"),
    (659, "HasVMScript"),
    (660, "GetVMScriptVariable"),
    (661, "GetWorkshopResourceDamage"),
    (664, "HasValidRumorTopic"),
    (672, "IsAttacking"),
    (673, "IsPowerAttacking"),
    (674, "IsLastHostileActor"),
    (675, "GetGraphVariableInt"),
    (678, "ShouldAttackKill"),
    (680, "GetActivationHeight"),
    (682, "WornHasKeyword"),
    (683, "GetPathingCurrentSpeed"),
    (684, "GetPathingCurrentSpeedAngle"),
    (691, "GetWorkshopObjectCount"),
    (693, "EPMagic_SpellHasKeyword"),
    (694, "GetNoBleedoutRecovery"),
    (696, "EPMagic_SpellHasSkill"),
    (697, "IsAttackType"),
    (698, "IsAllowedToFly"),
    (699, "HasMagicEffectKeyword"),
    (700, "IsCommandedActor"),
    (701, "IsStaggered"),
    (702, "IsRecoiling"),
    (703, "HasScopeWeaponEquipped"),
    (704, "IsPathing"),
    (705, "GetShouldHelp"),
    (706, "HasBoundWeaponEquipped"),
    (707, "GetCombatTargetHasKeyword"),
    (709, "GetCombatGroupMemberCount"),
    (710, "IsIgnoringCombat"),
    (711, "GetLightLevel"),
    (713, "SpellHasCastingPerk"),
    (714, "IsBeingRidden"),
    (715, "IsUndead"),
    (716, "GetRealHoursPassed"),
    (718, "IsUnlockedDoor"),
    (719, "IsHostileToActor"),
    (720, "GetTargetHeight"),
    (721, "IsPoison"),
    (722, "WornApparelHasKeywordCount"),
    (723, "GetItemHealthPercent"),
    (724, "EffectWasDualCast"),
    (725, "GetKnockStateEnum"),
    (726, "DoesNotExist"),
    (728, "GetPlayerWalkAwayFromDialogueScene"),
    (729, "GetActorStance"),
    (734, "CanProduceForWorkshop"),
    (735, "CanFlyHere"),
    (736, "EPIsDamageType"),
    (738, "GetActorGunState"),
    (739, "GetVoiceLineLength"),
    (741, "ObjectTemplateItem_HasKeyword"),
    (742, "ObjectTemplateItem_HasUniqueKeyword"),
    (743, "ObjectTemplateItem_GetLevel"),
    (744, "MovementIdleMatches"),
    (745, "GetActionData"),
    (746, "GetActionDataShort"),
    (747, "GetActionDataByte"),
    (748, "GetActionDataFlag"),
    (749, "ModdedItemHasKeyword"),
    (750, "GetAngryWithPlayer"),
    (751, "IsCameraUnderWater"),
    (753, "IsActorRefOwner"),
    (754, "HasActorRefOwner"),
    (756, "GetLoadedAmmoCount"),
    (757, "IsTimeSpanSunrise"),
    (758, "IsTimeSpanMorning"),
    (759, "IsTimeSpanAfternoon"),
    (760, "IsTimeSpanEvening"),
    (761, "IsTimeSpanSunset"),
    (762, "IsTimeSpanNight"),
    (763, "IsTimeSpanMidnight"),
    (764, "IsTimeSpanAnyDay"),
    (765, "IsTimeSpanAnyNight"),
    (766, "CurrentFurnitureHasKeyword"),
    (767, "GetWeaponEquipIndex"),
    (769, "IsOverEncumbered"),
    (770, "IsPackageRequestingBlockedIdles"),
    (771, "GetActionDataInt"),
    (772, "GetVATSRightMinusLeftAreaFree"),
    (773, "GetInIronSights"),
    (774, "GetActorStaggerDirection"),
    (775, "GetActorStaggerMagnitude"),
    (776, "WornCoversBipedSlot"),
    (777, "GetInventoryValue"),
    (778, "IsPlayerInConversation"),
    (779, "IsInDialogueCamera"),
    (780, "IsMyDialogueTargetPlayer"),
    (781, "IsMyDialogueTargetActor"),
    (782, "GetMyDialogueTargetDistance"),
    (783, "IsSeatOccupied"),
    (784, "IsPlayerRiding"),
    (785, "IsTryingEventCamera"),
    (786, "UseLeftSideCamera"),
    (787, "GetNoteType"),
    (788, "LocationHasPlayerOwnedWorkshop"),
    (789, "IsStartingAction"),
    (790, "IsMidAction"),
    (791, "IsWeaponChargeAttack"),
    (792, "IsInWorkshopMode"),
    (793, "IsWeaponChargingHoldAttack"),
    (794, "IsEncounterAbovePlayerLevel"),
    (795, "IsMeleeAttacking"),
    (796, "GetVATSQueuedTargetsUnique"),
    (797, "GetCurrentLocationCleared"),
    (798, "IsPowered"),
    (799, "GetTransmitterDistance"),
    (800, "GetCameraPlaybackTime"),
    (801, "IsInWater"),
    (802, "GetWithinActivateDistance"),
    (803, "IsUnderWater"),
    (804, "IsInSameSpace"),
    (805, "LocationAllowsReset"),
    (806, "GetVATSBackRightAreaFree"),
    (807, "GetVATSBackLeftAreaFree"),
    (808, "GetVATSBackRightTargetVisible"),
    (809, "GetVATSBackLeftTargetVisible"),
    (810, "GetVATSTargetLimbVisible"),
    (811, "IsPlayerListening"),
    (812, "GetPathingRequestedQuickTurn"),
    (813, "EPIsCalculatingBaseDamage"),
    (814, "GetReanimating"),
    (817, "IsInRobotWorkbench"),
];

#[derive(Debug, Clone, Copy)]
struct SchemaParameterTypes {
    types: [&'static str; 3],
    categories: [&'static str; 3],
}

const PARAMETER_TYPES: &[(u16, SchemaParameterTypes)] = &[
    (
        1,
        SchemaParameterTypes {
            types: ["ObjectReference", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        6,
        SchemaParameterTypes {
            types: ["Axis", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        8,
        SchemaParameterTypes {
            types: ["Axis", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        10,
        SchemaParameterTypes {
            types: ["Axis", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        11,
        SchemaParameterTypes {
            types: ["Axis", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        14,
        SchemaParameterTypes {
            types: ["ActorValue", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        27,
        SchemaParameterTypes {
            types: ["ObjectReference", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        32,
        SchemaParameterTypes {
            types: ["ObjectReference", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        36,
        SchemaParameterTypes {
            types: ["Integer", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        42,
        SchemaParameterTypes {
            types: ["Actor", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        43,
        SchemaParameterTypes {
            types: ["Actor", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        44,
        SchemaParameterTypes {
            types: ["Actor", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        45,
        SchemaParameterTypes {
            types: ["Actor", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        47,
        SchemaParameterTypes {
            types: ["ReferencableObject", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        56,
        SchemaParameterTypes {
            types: ["Quest", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        58,
        SchemaParameterTypes {
            types: ["Quest", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        59,
        SchemaParameterTypes {
            types: ["Quest", "QuestStage", "None"],
            categories: ["form", "number", "none"],
        },
    ),
    (
        60,
        SchemaParameterTypes {
            types: ["Faction", "Actor", "None"],
            categories: ["form", "form", "none"],
        },
    ),
    (
        66,
        SchemaParameterTypes {
            types: ["Actor", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        67,
        SchemaParameterTypes {
            types: ["Cell", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        68,
        SchemaParameterTypes {
            types: ["Class", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        69,
        SchemaParameterTypes {
            types: ["Race", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        70,
        SchemaParameterTypes {
            types: ["Sex", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        71,
        SchemaParameterTypes {
            types: ["Faction", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        72,
        SchemaParameterTypes {
            types: ["ReferencableObject", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        73,
        SchemaParameterTypes {
            types: ["Faction", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        74,
        SchemaParameterTypes {
            types: ["Global", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        79,
        SchemaParameterTypes {
            types: ["ObjectReference", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        84,
        SchemaParameterTypes {
            types: ["ActorBase", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        98,
        SchemaParameterTypes {
            types: ["Integer", "Integer", "Integer"],
            categories: ["number", "number", "number"],
        },
    ),
    (
        99,
        SchemaParameterTypes {
            types: ["ObjectReference", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        109,
        SchemaParameterTypes {
            types: ["ActorValue", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        117,
        SchemaParameterTypes {
            types: ["Region", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        122,
        SchemaParameterTypes {
            types: ["Actor", "CrimeType", "None"],
            categories: ["form", "number", "none"],
        },
    ),
    (
        131,
        SchemaParameterTypes {
            types: ["Scene", "Integer", "None"],
            categories: ["form", "number", "none"],
        },
    ),
    (
        132,
        SchemaParameterTypes {
            types: ["ObjectReference", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        136,
        SchemaParameterTypes {
            types: ["ObjectReference", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        142,
        SchemaParameterTypes {
            types: ["ReferencableObject", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        149,
        SchemaParameterTypes {
            types: ["Weather", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        152,
        SchemaParameterTypes {
            types: ["Faction", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        161,
        SchemaParameterTypes {
            types: ["Package", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        162,
        SchemaParameterTypes {
            types: ["ObjectReference", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        163,
        SchemaParameterTypes {
            types: ["Furniture", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        172,
        SchemaParameterTypes {
            types: ["Actor", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        180,
        SchemaParameterTypes {
            types: ["ObjectReference", "Keyword", "None"],
            categories: ["form", "form", "none"],
        },
    ),
    (
        181,
        SchemaParameterTypes {
            types: ["Alias", "Keyword", "None"],
            categories: ["number", "form", "none"],
        },
    ),
    (
        182,
        SchemaParameterTypes {
            types: ["ReferencableObject", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        193,
        SchemaParameterTypes {
            types: ["Faction", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        195,
        SchemaParameterTypes {
            types: ["Faction", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        197,
        SchemaParameterTypes {
            types: ["Faction", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        199,
        SchemaParameterTypes {
            types: ["Faction", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        214,
        SchemaParameterTypes {
            types: ["MagicEffect", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        223,
        SchemaParameterTypes {
            types: ["MagicItem", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        228,
        SchemaParameterTypes {
            types: ["Class", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        230,
        SchemaParameterTypes {
            types: ["Cell", "ObjectReference", "None"],
            categories: ["form", "form", "none"],
        },
    ),
    (
        246,
        SchemaParameterTypes {
            types: ["ReferencableObject", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        247,
        SchemaParameterTypes {
            types: ["FormType", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        248,
        SchemaParameterTypes {
            types: ["Scene", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        250,
        SchemaParameterTypes {
            types: ["Location", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        258,
        SchemaParameterTypes {
            types: ["Actor", "AssociationType", "None"],
            categories: ["form", "form", "none"],
        },
    ),
    (
        259,
        SchemaParameterTypes {
            types: ["Actor", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        261,
        SchemaParameterTypes {
            types: ["Actor", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        262,
        SchemaParameterTypes {
            types: ["FormList", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        264,
        SchemaParameterTypes {
            types: ["MagicItem", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        277,
        SchemaParameterTypes {
            types: ["ActorValue", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        278,
        SchemaParameterTypes {
            types: ["Owner", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        280,
        SchemaParameterTypes {
            types: ["Cell", "Owner", "None"],
            categories: ["form", "form", "none"],
        },
    ),
    (
        289,
        SchemaParameterTypes {
            types: ["Integer", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        310,
        SchemaParameterTypes {
            types: ["Worldspace", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        312,
        SchemaParameterTypes {
            types: ["MiscStat", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        325,
        SchemaParameterTypes {
            types: ["Packdata", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        359,
        SchemaParameterTypes {
            types: ["Location", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        360,
        SchemaParameterTypes {
            types: ["Alias", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        362,
        SchemaParameterTypes {
            types: ["Keyword", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        366,
        SchemaParameterTypes {
            types: ["Faction", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        368,
        SchemaParameterTypes {
            types: ["Integer", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        370,
        SchemaParameterTypes {
            types: ["Actor", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        372,
        SchemaParameterTypes {
            types: ["FormList", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        373,
        SchemaParameterTypes {
            types: ["Faction", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        375,
        SchemaParameterTypes {
            types: ["Faction", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        376,
        SchemaParameterTypes {
            types: ["Faction", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        378,
        SchemaParameterTypes {
            types: ["Actor", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        397,
        SchemaParameterTypes {
            types: ["Integer", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        398,
        SchemaParameterTypes {
            types: ["FormList", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        403,
        SchemaParameterTypes {
            types: ["Actor", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        407,
        SchemaParameterTypes {
            types: ["Integer", "Integer", "None"],
            categories: ["number", "number", "none"],
        },
    ),
    (
        408,
        SchemaParameterTypes {
            types: ["Actor", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        409,
        SchemaParameterTypes {
            types: ["FormList", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        410,
        SchemaParameterTypes {
            types: ["Faction", "Faction", "None"],
            categories: ["form", "form", "none"],
        },
    ),
    (
        414,
        SchemaParameterTypes {
            types: ["ObjectReference", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        426,
        SchemaParameterTypes {
            types: ["VoiceType", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        432,
        SchemaParameterTypes {
            types: ["FormType", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        437,
        SchemaParameterTypes {
            types: ["Integer", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        438,
        SchemaParameterTypes {
            types: ["ObjectReference", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        439,
        SchemaParameterTypes {
            types: ["ReferencableObject", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        444,
        SchemaParameterTypes {
            types: ["FormList", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        445,
        SchemaParameterTypes {
            types: ["EncounterZone", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        446,
        SchemaParameterTypes {
            types: ["Axis", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        447,
        SchemaParameterTypes {
            types: ["String", "None", "None"],
            categories: ["string", "none", "none"],
        },
    ),
    (
        448,
        SchemaParameterTypes {
            types: ["Perk", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        449,
        SchemaParameterTypes {
            types: ["Actor", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        450,
        SchemaParameterTypes {
            types: ["IdleForm", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        459,
        SchemaParameterTypes {
            types: ["Faction", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        463,
        SchemaParameterTypes {
            types: ["ObjectReference", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        465,
        SchemaParameterTypes {
            types: ["Keyword", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        473,
        SchemaParameterTypes {
            types: ["Alignment", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        477,
        SchemaParameterTypes {
            types: ["Actor", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        479,
        SchemaParameterTypes {
            types: ["EquipType", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        493,
        SchemaParameterTypes {
            types: ["ReferencableObject", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        494,
        SchemaParameterTypes {
            types: ["ActorValue", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        497,
        SchemaParameterTypes {
            types: ["Faction", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        501,
        SchemaParameterTypes {
            types: ["Keyword", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        506,
        SchemaParameterTypes {
            types: ["ActorValue", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        507,
        SchemaParameterTypes {
            types: ["Perk", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        510,
        SchemaParameterTypes {
            types: ["ReferencableObject", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        511,
        SchemaParameterTypes {
            types: ["ActorValue", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        512,
        SchemaParameterTypes {
            types: ["ObjectReference", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        513,
        SchemaParameterTypes {
            types: ["Actor", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        515,
        SchemaParameterTypes {
            types: ["ObjectReference", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        516,
        SchemaParameterTypes {
            types: ["ObjectReference", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        517,
        SchemaParameterTypes {
            types: ["ObjectReference", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        518,
        SchemaParameterTypes {
            types: ["ObjectReference", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        522,
        SchemaParameterTypes {
            types: ["ObjectReference", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        523,
        SchemaParameterTypes {
            types: ["ObjectReference", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        524,
        SchemaParameterTypes {
            types: ["ObjectReference", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        525,
        SchemaParameterTypes {
            types: ["ObjectReference", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        528,
        SchemaParameterTypes {
            types: ["CriticalStage", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        533,
        SchemaParameterTypes {
            types: ["Faction", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        534,
        SchemaParameterTypes {
            types: ["Faction", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        535,
        SchemaParameterTypes {
            types: ["Faction", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        543,
        SchemaParameterTypes {
            types: ["Quest", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        550,
        SchemaParameterTypes {
            types: ["Scene", "Integer", "None"],
            categories: ["form", "number", "none"],
        },
    ),
    (
        552,
        SchemaParameterTypes {
            types: ["MagicItem", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        560,
        SchemaParameterTypes {
            types: ["Keyword", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        561,
        SchemaParameterTypes {
            types: ["RefType", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        562,
        SchemaParameterTypes {
            types: ["Keyword", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        563,
        SchemaParameterTypes {
            types: ["RefType", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        565,
        SchemaParameterTypes {
            types: ["Location", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        566,
        SchemaParameterTypes {
            types: ["Alias", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        567,
        SchemaParameterTypes {
            types: ["Alias", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        570,
        SchemaParameterTypes {
            types: ["CastingSource", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        571,
        SchemaParameterTypes {
            types: ["CastingSource", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        572,
        SchemaParameterTypes {
            types: ["CastingSource", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        576,
        SchemaParameterTypes {
            types: ["Event", "EventData", "String"],
            categories: ["number", "form", "string"],
        },
    ),
    (
        577,
        SchemaParameterTypes {
            types: ["ObjectReference", "ObjectReference", "None"],
            categories: ["form", "form", "none"],
        },
    ),
    (
        584,
        SchemaParameterTypes {
            types: ["ObjectReference", "Axis", "None"],
            categories: ["form", "number", "none"],
        },
    ),
    (
        591,
        SchemaParameterTypes {
            types: ["Location", "RefType", "None"],
            categories: ["form", "form", "none"],
        },
    ),
    (
        592,
        SchemaParameterTypes {
            types: ["Location", "RefType", "None"],
            categories: ["form", "form", "none"],
        },
    ),
    (
        595,
        SchemaParameterTypes {
            types: ["MagicItem", "CastingSource", "None"],
            categories: ["form", "number", "none"],
        },
    ),
    (
        596,
        SchemaParameterTypes {
            types: ["CastingSource", "Keyword", "None"],
            categories: ["number", "form", "none"],
        },
    ),
    (
        597,
        SchemaParameterTypes {
            types: ["CastingSource", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        598,
        SchemaParameterTypes {
            types: ["Alias", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        600,
        SchemaParameterTypes {
            types: ["Alias", "RefType", "None"],
            categories: ["number", "form", "none"],
        },
    ),
    (
        601,
        SchemaParameterTypes {
            types: ["Alias", "RefType", "None"],
            categories: ["number", "form", "none"],
        },
    ),
    (
        602,
        SchemaParameterTypes {
            types: ["WardState", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        603,
        SchemaParameterTypes {
            types: ["ObjectReference", "Keyword", "None"],
            categories: ["form", "form", "none"],
        },
    ),
    (
        604,
        SchemaParameterTypes {
            types: ["Alias", "Keyword", "None"],
            categories: ["number", "form", "none"],
        },
    ),
    (
        605,
        SchemaParameterTypes {
            types: ["Alias", "Location", "None"],
            categories: ["number", "form", "none"],
        },
    ),
    (
        606,
        SchemaParameterTypes {
            types: ["Location", "Keyword", "None"],
            categories: ["form", "form", "none"],
        },
    ),
    (
        608,
        SchemaParameterTypes {
            types: ["Alias", "Keyword", "None"],
            categories: ["number", "form", "none"],
        },
    ),
    (
        610,
        SchemaParameterTypes {
            types: ["Alias", "Keyword", "None"],
            categories: ["number", "form", "none"],
        },
    ),
    (
        611,
        SchemaParameterTypes {
            types: ["Packdata", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        612,
        SchemaParameterTypes {
            types: ["Packdata", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        617,
        SchemaParameterTypes {
            types: ["AssociationType", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        619,
        SchemaParameterTypes {
            types: ["Axis", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        620,
        SchemaParameterTypes {
            types: ["Axis", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        622,
        SchemaParameterTypes {
            types: ["Axis", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        624,
        SchemaParameterTypes {
            types: ["ObjectReference", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        625,
        SchemaParameterTypes {
            types: ["Location", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        626,
        SchemaParameterTypes {
            types: ["Alias", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        629,
        SchemaParameterTypes {
            types: ["Quest", "String", "None"],
            categories: ["form", "string", "none"],
        },
    ),
    (
        637,
        SchemaParameterTypes {
            types: ["ReferencableObject", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        639,
        SchemaParameterTypes {
            types: ["ObjectReference", "Float", "None"],
            categories: ["form", "number", "none"],
        },
    ),
    (
        640,
        SchemaParameterTypes {
            types: ["ActorValue", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        650,
        SchemaParameterTypes {
            types: ["ObjectReference", "Keyword", "None"],
            categories: ["form", "form", "none"],
        },
    ),
    (
        651,
        SchemaParameterTypes {
            types: ["Keyword", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        652,
        SchemaParameterTypes {
            types: ["ObjectReference", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        659,
        SchemaParameterTypes {
            types: ["String", "None", "None"],
            categories: ["string", "none", "none"],
        },
    ),
    (
        660,
        SchemaParameterTypes {
            types: ["String", "String", "None"],
            categories: ["string", "string", "none"],
        },
    ),
    (
        661,
        SchemaParameterTypes {
            types: ["ActorValue", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        664,
        SchemaParameterTypes {
            types: ["Quest", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        675,
        SchemaParameterTypes {
            types: ["String", "None", "None"],
            categories: ["string", "none", "none"],
        },
    ),
    (
        678,
        SchemaParameterTypes {
            types: ["Actor", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        682,
        SchemaParameterTypes {
            types: ["Keyword", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        684,
        SchemaParameterTypes {
            types: ["Axis", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        691,
        SchemaParameterTypes {
            types: ["ReferencableObject", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        693,
        SchemaParameterTypes {
            types: ["Keyword", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        696,
        SchemaParameterTypes {
            types: ["ActorValue", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        697,
        SchemaParameterTypes {
            types: ["Keyword", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        699,
        SchemaParameterTypes {
            types: ["Keyword", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        705,
        SchemaParameterTypes {
            types: ["Actor", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        706,
        SchemaParameterTypes {
            types: ["CastingSource", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        707,
        SchemaParameterTypes {
            types: ["Keyword", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        713,
        SchemaParameterTypes {
            types: ["Perk", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        719,
        SchemaParameterTypes {
            types: ["Actor", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        720,
        SchemaParameterTypes {
            types: ["ObjectReference", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        722,
        SchemaParameterTypes {
            types: ["Keyword", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        736,
        SchemaParameterTypes {
            types: ["DamageType", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        741,
        SchemaParameterTypes {
            types: ["Keyword", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        742,
        SchemaParameterTypes {
            types: ["Keyword", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        744,
        SchemaParameterTypes {
            types: ["Integer", "Integer", "None"],
            categories: ["number", "number", "none"],
        },
    ),
    (
        746,
        SchemaParameterTypes {
            types: ["Integer", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        747,
        SchemaParameterTypes {
            types: ["Integer", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        748,
        SchemaParameterTypes {
            types: ["Integer", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        749,
        SchemaParameterTypes {
            types: ["Keyword", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        753,
        SchemaParameterTypes {
            types: ["Actor", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        754,
        SchemaParameterTypes {
            types: ["Actor", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        766,
        SchemaParameterTypes {
            types: ["Keyword", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        772,
        SchemaParameterTypes {
            types: ["ObjectReference", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        773,
        SchemaParameterTypes {
            types: ["ObjectReference", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        776,
        SchemaParameterTypes {
            types: ["Integer", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
    (
        783,
        SchemaParameterTypes {
            types: ["Keyword", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        802,
        SchemaParameterTypes {
            types: ["ObjectReference", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        804,
        SchemaParameterTypes {
            types: ["ObjectReference", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        806,
        SchemaParameterTypes {
            types: ["ObjectReference", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        807,
        SchemaParameterTypes {
            types: ["ObjectReference", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        808,
        SchemaParameterTypes {
            types: ["ObjectReference", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        809,
        SchemaParameterTypes {
            types: ["ObjectReference", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        810,
        SchemaParameterTypes {
            types: ["ObjectReference", "None", "None"],
            categories: ["form", "none", "none"],
        },
    ),
    (
        811,
        SchemaParameterTypes {
            types: ["Float", "None", "None"],
            categories: ["number", "none", "none"],
        },
    ),
];

pub(super) fn function_parameter_hint(function_index: u16) -> FunctionParameterHint {
    let function_name = FUNCTION_NAMES
        .binary_search_by_key(&function_index, |(index, _)| *index)
        .ok()
        .map(|position| FUNCTION_NAMES[position].1);
    let parameter_types = PARAMETER_TYPES
        .binary_search_by_key(&function_index, |(index, _)| *index)
        .ok()
        .map(|position| PARAMETER_TYPES[position].1);
    let (mapping_status, types, categories) = match parameter_types {
        Some(mapping) => ("explicit", mapping.types, mapping.categories),
        None if function_name.is_some() => (
            "function-enum-known-parameter-map-defaulted",
            ["Unspecified"; 3],
            ["unresolved"; 3],
        ),
        None => (
            "unknown-function-index",
            ["Unspecified"; 3],
            ["unresolved"; 3],
        ),
    };
    FunctionParameterHint {
        function_name,
        mapping_status,
        parameter_one_type: types[0],
        parameter_one_category: categories[0],
        parameter_two_type: types[1],
        parameter_two_category: categories[1],
        parameter_three_type: types[2],
        parameter_three_category: categories[2],
    }
}

pub(super) fn function_schema_rows() -> impl Iterator<Item = (u16, FunctionParameterHint)> {
    FUNCTION_NAMES
        .iter()
        .map(|(function_index, _)| (*function_index, function_parameter_hint(*function_index)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_function_tables_are_complete_and_sorted() {
        assert_eq!(FUNCTION_NAMES.len(), 479);
        assert_eq!(PARAMETER_TYPES.len(), 220);
        assert!(FUNCTION_NAMES.windows(2).all(|rows| rows[0].0 < rows[1].0));
        assert!(PARAMETER_TYPES.windows(2).all(|rows| rows[0].0 < rows[1].0));
    }

    #[test]
    fn pinned_function_names_types_and_defaulted_states_are_retained() {
        let distance = function_parameter_hint(1);
        assert_eq!(distance.function_name, Some("GetDistance"));
        assert_eq!(distance.parameter_one_type, "ObjectReference");
        assert_eq!(distance.parameter_one_category, "form");
        assert_eq!(distance.parameter_two_category, "none");
        assert_eq!(distance.parameter_three_category, "none");

        let two_form_parameters = function_parameter_hint(60);
        assert_eq!(
            two_form_parameters.function_name,
            Some("GetFactionRankDifference")
        );
        assert_eq!(two_form_parameters.parameter_two_type, "Actor");
        assert_eq!(two_form_parameters.parameter_two_category, "form");

        let third_string_parameter = function_parameter_hint(576);
        assert_eq!(third_string_parameter.function_name, Some("GetEventData"));
        assert_eq!(third_string_parameter.parameter_three_type, "String");
        assert_eq!(third_string_parameter.parameter_three_category, "string");

        let defaulted = function_parameter_hint(300);
        assert_eq!(defaulted.function_name, Some("IsInInterior"));
        assert_eq!(
            defaulted.mapping_status,
            "function-enum-known-parameter-map-defaulted"
        );
        assert_eq!(defaulted.parameter_two_category, "unresolved");
        assert_eq!(defaulted.parameter_three_category, "unresolved");

        let unknown = function_parameter_hint(9000);
        assert_eq!(unknown.function_name, None);
        assert_eq!(unknown.mapping_status, "unknown-function-index");
        assert_eq!(unknown.parameter_one_category, "unresolved");
    }
}
