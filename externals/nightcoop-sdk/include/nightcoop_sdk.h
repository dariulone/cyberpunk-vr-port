/* SPDX-License-Identifier: MIT — Copyright (c) 2026 NightCoop (see sdk/LICENSE) */
/*
 * NightCoop SDK — the public C interface of the NightCoop co-op mod for other Cyberpunk 2077 mods.
 *
 * Version 1. One header, plain C: numbers, UTF-8 strings (zero-terminated), byte buffers and
 * function pointers. No C++ types cross the boundary, so a mod built with any compiler can use it.
 *
 * Who uses it
 *   - RED4ext plugins (C/C++): this header directly.
 *   - redscript mods: the native class NightCoopSdk (sdk/redscript/NightCoop/NightCoopSdkNative.reds, top level; the enums
 *     are in NightCoopSdk.reds, module NightCoop); guard calls with @if(ModuleExists("NightCoop")) so the mod still
 *     compiles and runs without NightCoop. NightCoop.dll adds both files to the redscript compilation itself — do not
 *     ship them with your mod.
 *   - Cyber Engine Tweaks (Lua): the same class, NightCoopSdk.Xxx(...), from the CET console or a CET mod.
 *   All three layers reach the same core; a mod may mix them.
 *
 * Connecting (RED4ext plugin)
 *   1. Find NightCoop: GetModuleHandleW(L"NightCoop.dll"); take the export NCSDK_EXPORT_NAME as NcSdkGetFn.
 *      Plugins load in an unknown order: look it up lazily (first game-thread tick, or a RED4ext "Running" state
 *      callback), not in your DllMain or Main(Load).
 *   2. api = get(NCSDK_VERSION). NULL means: NightCoop has no interface of this version, or the player switched the SDK
 *      off (switch "sdk"). Work as if NightCoop were absent.
 *   3. mod = api->registerMod(&info). 0 means refused (bad id, id already registered, too many mods); see the log.
 *   4. Call the table with your handle. Unregister on unload (api->unregisterMod). If NightCoop unloads first it calls
 *      hooks.shutdown: stop calling the table, its memory goes away with NightCoop.dll.
 *
 * Versions
 *   NCSDK_VERSION changes only on an incompatible change. New functions go only to the end of NcSdkApi, new callbacks
 *   only to the end of NcSdkHooks, new fields only to the end of structs that carry a size field. Check a newer
 *   function with NCSDK_HAS(api, field) before calling it.
 *
 * Threads
 *   Everything is the game thread (where RED4ext game-state updates, script natives and NightCoop's frame run) unless
 *   a function says "any thread". Callbacks come on the game thread. Do not block in callbacks.
 *
 * Sessions and authority (what every co-op mod has to know)
 *   - A session has one host (peer 0) and guests. The host's game is the world: time, weather, NPCs, quests, the
 *     population. A guest who changes the world locally diverges from everyone else.
 *   - Every player leads their own player, their own driven vehicle and the NPCs NightCoop gave them; the others see
 *     copies ("mirrors") of those. NcSdkApi.entityIsMine tells which is which; change only what is yours, ask the
 *     owner (a channel message) for the rest.
 *   - Mod data travels only between players who registered the same mod id with the same NcSdkModInfo.protocol.
 *     A partner without your mod simply receives nothing; NcSdkApi.peerHasMod tells you.
 *
 * Results: functions returning int32_t give NCSDK_OK (0) or a positive count/flag on success, a negative NCSDK_E_*
 * on failure, unless they say otherwise.
 */
#pragma once

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/** Export of NightCoop.dll: const NcSdkApi* NightCoopSdkGet(uint32_t aVersion). */
#define NCSDK_EXPORT_NAME "NightCoopSdkGet"
#define NCSDK_VERSION 1u

/** Is the field present in this table (the table may come from an older NightCoop of the same version)? */
#define NCSDK_HAS(aApi, aField) \
    ((aApi) && (aApi)->size >= offsetof(NcSdkApi, aField) + sizeof((aApi)->aField) && (aApi)->aField)

/* ------------------------------------------------------------------------------------------------ basic types */

/** A registered mod (returned by registerMod). 0 — none. */
typedef uint32_t NcSdkMod;
/** A player of the session. The host is always 0. */
typedef uint32_t NcSdkPeer;

#define NCSDK_PEER_HOST 0u
#define NCSDK_PEER_NONE 0xFFFFFFFFu
/** send(): every peer of the session that has the mod (except yourself). */
#define NCSDK_TO_ALL 0xFFFFFFFEu

enum
{
    NCSDK_OK = 0,
    NCSDK_E_ARG = -1,         /**< bad argument (NULL, empty, too long, bad handle) */
    NCSDK_E_STATE = -2,       /**< not now: no session, not in the world, the player is loading */
    NCSDK_E_LIMIT = -3,       /**< size or rate limit of the mod reached; try later, send less */
    NCSDK_E_UNSUPPORTED = -4, /**< this NightCoop build has no such feature */
    NCSDK_E_OFF = -5,         /**< the player switched this feature off */
    NCSDK_E_DENIED = -6,      /**< not yours (not the owner), or the host refused */
    NCSDK_E_UNKNOWN = -7,     /**< no such peer, entity, key or action */
    NCSDK_E_SIZE = -8,        /**< the output buffer is too small (the needed size is returned where documented) */
};

/** Log levels. */
enum
{
    NCSDK_LOG_TRACE = 0,
    NCSDK_LOG_INFO = 1,
    NCSDK_LOG_WARN = 2,
    NCSDK_LOG_ERROR = 3,
};

/** On-screen message tone (how loud it is). */
enum
{
    NCSDK_TONE_QUIET = 0,
    NCSDK_TONE_INFO = 1,
    NCSDK_TONE_WAIT = 2,
    NCSDK_TONE_URGENT = 3,
};

typedef struct NcSdkVec3
{
    float x;
    float y;
    float z;
} NcSdkVec3;

/** Quaternion, the game's order (i, j, k, r). */
typedef struct NcSdkQuat
{
    float i;
    float j;
    float k;
    float r;
} NcSdkQuat;

typedef struct NcSdkTransform
{
    NcSdkVec3 pos;
    NcSdkQuat rot;
} NcSdkTransform;

/* ------------------------------------------------------------------------------------------------ limits */

#define NCSDK_ID_MAX 64           /**< mod id: ASCII [a-z0-9._-], 3..64 bytes, e.g. "jackhumbert.let_there_be_flight" */
#define NCSDK_STATE_KEY_MAX 64    /**< shared state key, UTF-8 bytes */
#define NCSDK_UNRELIABLE_MAX 1024 /**< payload of an unreliable message or entity state stream, bytes */
#define NCSDK_RELIABLE_MAX 16384  /**< payload of a reliable message, bytes */
#define NCSDK_STATE_VALUE_MAX 4096
#define NCSDK_STATE_KEYS_MAX 256  /**< shared state keys per mod */
#define NCSDK_ENTITY_STATE_MAX 1024
/** Rate per mod and direction, averaged over a second; bursts up to 4x for a moment. Over it — NCSDK_E_LIMIT. */
#define NCSDK_RATE_BYTES_PER_S 32768
#define NCSDK_RATE_MESSAGES_PER_S 120

/* ------------------------------------------------------------------------------------------------ mod passport */

enum
{
    /** Every player of a session should have it: the mods check warns both sides when a partner lacks it. */
    NCSDK_MOD_NEEDED_BY_ALL = 1u << 0,
    /** It changes only its own player (looks, HUD, controls): partners need nothing. */
    NCSDK_MOD_LOCAL_ONLY = 1u << 1,
    /** It changes the shared world (time, weather, NPCs, objects, the population). */
    NCSDK_MOD_CHANGES_WORLD = 1u << 2,
    /** It changes quests, facts or scenes. */
    NCSDK_MOD_CHANGES_QUESTS = 1u << 3,
    /** It adds items (TweakDB records) a partner without it does not have. */
    NCSDK_MOD_ADDS_ITEMS = 1u << 4,
    /** It spawns its own entities (NPCs, props, vehicles). */
    NCSDK_MOD_SPAWNS_ENTITIES = 1u << 5,
    /** Its world changes come from the host: guests ask the host by channel or shared state. */
    NCSDK_MOD_HOST_AUTHORITY = 1u << 6,

    /* What a mod uses (sdk/SECURITY.md). Declare it here (the partners see it in the
     * passport); a use that is not declared answers NCSDK_E_DENIED (one log line), and partners' data of that kind for
     * the mod is dropped. The player can switch the check off ("sdk_permissions"). Other uses need the flags above:
     * entityShare / entityUnshare / entityKeepLocal — NCSDK_MOD_SPAWNS_ENTITIES, factKeepLocal — NCSDK_MOD_CHANGES_QUESTS.
     * Reading (where, peers, entityRef, entityOfRef, entityIsMine, partnerBody, partnerPose, actionList,
     * actionAvailable), log, showMessage, pick, cleanText and pollEvent need nothing. */
    NCSDK_MOD_USES_CHANNELS = 1u << 8,      /**< send(); partners' channel messages to the mod */
    NCSDK_MOD_USES_SHARED_STATE = 1u << 9,  /**< stateSet, stateGet; partners' shared state of the mod */
    NCSDK_MOD_USES_ENTITY_STATE = 1u << 10, /**< entitySetState, entityClearState, entityAirborne; partners' entity state */
    NCSDK_MOD_USES_POSE = 1u << 11,         /**< poseSet, poseClear; partnerPose callbacks and events */
    NCSDK_MOD_USES_ACTIONS = 1u << 12,      /**< actionInvoke */

    /* sdk/docs/npc-effects.md */
    NCSDK_MOD_USES_NPC_REQUESTS = 1u << 13, /**< npcEffect, npcRequest, npcReply; partners' requests about NPCs we lead
                                                 and the answers to ours */
    NCSDK_MOD_USES_WORLD_TIME = 1u << 26,   /**< worldSlow */
};

/* What a mod uses — fallbackSet (setting a fallback; clearing one needs nothing).
 * contentHere and peerHasKnownMod only read and need nothing. */
enum
{
    NCSDK_MOD_USES_FALLBACKS = 1u << 14, /**< fallbackSet: name what a partner without the mod sees instead of its content */
};

/** Session events (hooks.event, script event kind 1..). */
enum
{
    NCSDK_EV_SESSION_START = 1,  /**< a session came up (peer: our own number) */
    NCSDK_EV_SESSION_END = 2,    /**< the session is over (or we left it) */
    NCSDK_EV_PEER_JOIN = 3,      /**< a player joined the session */
    NCSDK_EV_PEER_LEAVE = 4,     /**< a player left (or the link is lost for good) */
    NCSDK_EV_PEER_WORLD_IN = 5,  /**< the player is in our world now (both are in the host's world) */
    NCSDK_EV_PEER_WORLD_OUT = 6, /**< the player left our world (menu, loading, own campaign) */
    NCSDK_EV_PEER_MOD = 7,       /**< the player has this mod (same id and protocol): channels to them work now */
    NCSDK_EV_PEER_NO_MOD = 8,    /**< the player answered the mods check without this mod (or another protocol) */
};

typedef struct NcSdkWhere
{
    uint32_t size;      /**< set by the caller: sizeof(NcSdkWhere) */
    uint8_t inSession;  /**< a session is up (connected; the host — even with no guests yet) */
    uint8_t isHost;     /**< we are the host (or alone, out of session: 0) */
    uint8_t inWorld;    /**< our player is in the world (not the main menu, not loading) */
    uint8_t inHostWorld;/**< we are in the host's world (a guest who joined it; the host — always when in world) */
    NcSdkPeer localPeer;/**< our number in the session; NCSDK_PEER_NONE out of session */
    uint32_t peerCount; /**< other players in the session */
    uint64_t sharedTimeMs; /**< the session clock, ms, the same on every machine (0 out of session) */
} NcSdkWhere;

/* ------------------------------------------------------------------------------------------------ entities */

/** Network reference: the same entity on every machine of the session (its mirror or copy there). */
enum
{
    NCSDK_NET_NONE = 0,
    NCSDK_NET_PLAYER = 1,  /**< a player's character; id = owner = the peer */
    NCSDK_NET_VEHICLE = 2, /**< a vehicle NightCoop syncs (driven or shared): id — its net id */
    NCSDK_NET_NPC = 3,     /**< an NPC NightCoop syncs: id — its net id */
    NCSDK_NET_SPAWN = 4,   /**< an entity a mod shared with entityShare: id — its share id */
};

typedef struct NcSdkNetRef
{
    uint8_t kind; /**< NCSDK_NET_* */
    uint8_t reserved[3];
    NcSdkPeer owner; /**< who leads it now */
    uint32_t id;
    uint32_t reserved2;
} NcSdkNetRef;

/** For scripts: NcSdkNetRef packed into 64 bits — kind 8 | owner 24 | id 32. */
#define NCSDK_PACK_REF(aKind, aOwner, aId) \
    ((((uint64_t)(aKind) & 0xFFu) << 56) | (((uint64_t)(aOwner) & 0xFFFFFFu) << 32) | (uint64_t)(uint32_t)(aId))

enum
{
    /** entitySetState: unreliable stream, only the newest value matters (positions, poses). Default — reliable. */
    NCSDK_STATE_STREAM = 1u << 0,
};

enum
{
    /** entityShare: partners get a copy that follows the owner's transform (default — placed once, then static). */
    NCSDK_SHARE_FOLLOW = 1u << 0,
    /** entityShare: an NPC copy should play the owner's animation stream as NightCoop's NPC copies do. */
    NCSDK_SHARE_ANIMATE = 1u << 1,
};

/* ------------------------------------------------------------------------------------------------ pose source */

/**
 * A pose source for the local player's body as partners see it: VR head and hands, motion capture, emote mods.
 * NightCoop sends it (~30 Hz) and lays it on our body on the partners' machines: neck and head by `head`, two-bone
 * IK of the arms to the hands, finger curls; the game's own graph stays for whatever is not valid.
 */
enum
{
    NCSDK_POSE_HEAD = 1u << 0,
    NCSDK_POSE_LEFT_HAND = 1u << 1,
    NCSDK_POSE_RIGHT_HAND = 1u << 2,
    NCSDK_POSE_FINGERS = 1u << 3,
    NCSDK_POSE_AIM = 1u << 4,    /**< aimOrigin/aimDir: where the weapon really points (not the camera) */
    NCSDK_POSE_CROUCH = 1u << 5, /**< physicalCrouch is set */
    /** Transforms are in world space at the time of the call; NightCoop converts them to body space itself.
     *  Without it they are in body space: origin at the entity position, axes of the entity orientation, metres. */
    NCSDK_POSE_WORLD_SPACE = 1u << 31,
};

typedef struct NcSdkPose
{
    uint32_t size;  /**< sizeof(NcSdkPose) */
    uint32_t valid; /**< NCSDK_POSE_* */
    NcSdkTransform head;
    NcSdkTransform leftHand;
    NcSdkTransform rightHand;
    float fingers[10];   /**< curl 0..1: left thumb, index, middle, ring, little; then right in the same order;
                              a negative value — that finger is not tracked, the game's graph keeps it */
    NcSdkVec3 aimOrigin; /**< world space always */
    NcSdkVec3 aimDir;    /**< world space always, unit length */
    float physicalCrouch;/**< 0 standing .. 1 fully down */
} NcSdkPose;

/* ------------------------------------------------------------------------------------------------ callbacks */

/* Defined below, before the table; the callbacks at the end of NcSdkHooks take them */
struct NcSdkCoopEvent;
struct NcSdkVoteResult;

typedef struct NcSdkHooks
{
    uint32_t size;  /**< sizeof(NcSdkHooks) of the mod */
    void* context;  /**< passed back as the first argument of every callback */

    /** Every game frame while NightCoop runs, after its own tick. aDt — seconds. */
    void (*frame)(void* aContext, float aDt);
    /** NCSDK_EV_*; aPeer — the peer it is about (NCSDK_PEER_NONE for the session itself). */
    void (*event)(void* aContext, int32_t aEvent, NcSdkPeer aPeer);
    /** A channel message of this mod from aFrom. */
    void (*message)(void* aContext, NcSdkPeer aFrom, uint16_t aChannel, const uint8_t* aData, size_t aSize);
    /** A shared state key changed (also once per key after joining a session). aSize 0 with aData NULL — removed. */
    void (*stateChanged)(void* aContext, const char* aKey, const uint8_t* aData, size_t aSize, NcSdkPeer aBy);
    /** Host only: a guest asks to set a key. Return 1 to accept, 0 to refuse. NULL — accept everything. */
    int32_t (*stateRequest)(void* aContext, NcSdkPeer aFrom, const char* aKey, const uint8_t* aData, size_t aSize);
    /** Entity state of this mod from the entity's owner, laid on our local mirror/copy aEntity (EntityID hash). Comes
     *  again when the mirror appears later (NightCoop keeps the last value per key while the entity lives).
     *  aSize 0 with aData NULL — the key was cleared. */
    void (*entityState)(void* aContext, NcSdkPeer aFrom, uint64_t aEntity, const NcSdkNetRef* aRef, uint16_t aKey,
                        const uint8_t* aData, size_t aSize);
    /** A partner's pose source changed (for mods that draw something themselves; NightCoop already lays it on the
     *  body). aPose NULL — the source is gone. */
    void (*partnerPose)(void* aContext, NcSdkPeer aPeer, const NcSdkPose* aPose);
    /** A shared spawn of a partner appeared here (aPresent 1, aEntity — our copy) or is gone (0). */
    void (*sharedSpawn)(void* aContext, NcSdkPeer aOwner, const NcSdkNetRef* aRef, uint64_t aEntity,
                        int32_t aPresent);
    /** NightCoop is unloading (game exit): stop, then never call the table again. */
    void (*shutdown)(void* aContext);

    /* Requests about NPCs (sdk/docs/npc-effects.md). */
    /** We lead the NPC aEntity (our local one) and aFrom asked this mod to do something on it (npcRequest of their copy of
     *  the mod; aFrom may be ourselves). Do it with the game here, then answer with npcReply(aRequest, ...). */
    void (*npcRequest)(void* aContext, NcSdkPeer aFrom, uint64_t aEntity, const NcSdkNetRef* aRef, uint32_t aRequest,
                       uint16_t aChannel, const uint8_t* aData, size_t aSize);
    /** The answer to our request aRequest (a positive return of npcEffect or npcRequest): aResult NCSDK_OK or NCSDK_E_*,
     *  aEffect — NCSDK_NPC_* of npcEffect (0 — a general npcRequest; aData — the owner mod's reply), aEntity — our local
     *  copy of the NPC now (0 — not here any more), aOwner — who answered. */
    void (*npcResult)(void* aContext, NcSdkPeer aOwner, uint64_t aEntity, const NcSdkNetRef* aRef, uint32_t aRequest,
                      int32_t aEffect, int32_t aResult, const uint8_t* aData, size_t aSize);
    /* --- co-op events with their details, the co-op menu, votes */
    /** A co-op event NCSDK_EV_PEER_DOWN .. NCSDK_EV_TIME_SKIP with its value, entity and reference. Set — these events
     *  come here and not to `event`; NULL — they come to `event` (event and peer only) or the queue. */
    void (*coopEvent)(void* aContext, const struct NcSdkCoopEvent* aEvent);
    /** The player pressed an item of this mod in NightCoop's co-op menu (menuItemSet). On the game thread. */
    void (*menuItem)(void* aContext, const char* aKey);
    /** A vote of this mod is over (voteStart) — on every machine of the session that has the mod. */
    void (*voteResult)(void* aContext, const struct NcSdkVoteResult* aResult);
    /** Host only: a guest's copy of this mod asks to change the world (worldAsk without NCSDK_WORLD_ASK_VOTE). Return 1 to
     *  do it (NightCoop sets it), 0 to refuse. NULL — such asks go to a vote of every player instead. On the game thread. */
    int32_t (*worldRequest)(void* aContext, NcSdkPeer aFrom, int32_t aKind, int32_t aValue, const char* aText);
    /* --- a shared spawn changed its leader. A NULL callback, or a hooks struct without this field (its size is
     * shorter), gets the item in the poll queue (NCSDK_POLL_SPAWN_LEADER). */
    /** The machine that leads the shared spawn aRef changed: an NPC shared with NCSDK_SHARE_HANDOFF went to the player
     *  nearest to it (or back), a mod vehicle got a new driver. aLeader — who leads it now; aMine 1 — this machine does
     *  (aEntity is our body of it), 0 — a partner does (ours is its mirror). Values of NcSdkApi.entitySetState stay on
     *  every machine until the new leader sets its own. */
    void (*spawnLeader)(void* aContext, NcSdkPeer aLeader, const NcSdkNetRef* aRef, uint64_t aEntity, int32_t aMine);
    /** The answer to our rewardGrant aGrant from aPeer: aResult NCSDK_OK — given; NCSDK_E_UNKNOWN — no such item record
     *  there; NCSDK_E_DENIED — refused (they lack your mod, your passport lacks NCSDK_MOD_USES_REWARDS, a quest item, too
     *  far); NCSDK_E_LIMIT — over the quantity or rate; NCSDK_E_STATE — not in one world, or no answer for a minute;
     *  NCSDK_E_OFF — they switched rewards off. Queued as NCSDK_POLL_REWARD when NULL. */
    void (*reward)(void* aContext, NcSdkPeer aPeer, uint64_t aGrant, int32_t aResult);
} NcSdkHooks;

typedef struct NcSdkModInfo
{
    uint32_t size;        /**< sizeof(NcSdkModInfo) */
    uint32_t flags;       /**< NCSDK_MOD_* */
    const char* id;       /**< stable unique id, see NCSDK_ID_MAX (copied) */
    const char* name;     /**< display name, UTF-8 (copied) */
    const char* version;  /**< the mod's version string, shown in the mods check (copied) */
    uint32_t protocol;    /**< the mod's own wire version: data goes only between equal numbers */
    uint32_t reserved;
    const NcSdkHooks* hooks; /**< may be NULL: script-style polling (pollEvent) instead (copied) */
} NcSdkModInfo;

/* ------------------------------------------------------------------------------------------------ events for polling */

/** What pollEvent returns: the same things the hooks deliver, one at a time (for scripts, CET, or C without hooks). */
enum
{
    NCSDK_POLL_EVENT = 1,        /**< a = NCSDK_EV_*, peer */
    NCSDK_POLL_MESSAGE = 2,      /**< peer = from, a = channel, data */
    NCSDK_POLL_STATE = 3,        /**< peer = by, key, data (size 0 — removed) */
    NCSDK_POLL_ENTITY_STATE = 4, /**< peer = from, entity, ref, a = key, data */
    NCSDK_POLL_PARTNER_POSE = 5, /**< peer; pose valid = 0 — gone */
    NCSDK_POLL_SHARED_SPAWN = 6, /**< peer = owner, entity, ref, a = present */
    /* the fields request and effect of NcSdkPolled carry the request; key is
     * "<request>/<effect>" in decimal (for text polling) */
    NCSDK_POLL_NPC_REQUEST = 7,  /**< peer = from, entity = our NPC, ref, a = channel, request, data (hooks.npcRequest) */
    NCSDK_POLL_NPC_RESULT = 8,   /**< peer = owner, entity = our copy, ref, a = result, request, effect, data */
};

typedef struct NcSdkPolled
{
    uint32_t size; /**< set by the caller: sizeof(NcSdkPolled) */
    int32_t kind;  /**< NCSDK_POLL_*; 0 — the queue is empty */
    NcSdkPeer peer;
    int32_t a;
    uint64_t entity;
    NcSdkNetRef ref;
    char key[NCSDK_STATE_KEY_MAX + 1];
    uint32_t dataSize; /**< the full size; data is cut at dataCap of the caller */
    uint8_t* data;     /**< caller's buffer, may be NULL */
    uint32_t dataCap;
    NcSdkPose pose;
    uint32_t request; /**< NCSDK_POLL_NPC_REQUEST / NCSDK_POLL_NPC_RESULT: the request number */
    int32_t effect;   /**< NCSDK_POLL_NPC_RESULT: NCSDK_NPC_* of npcEffect, 0 — a general npcRequest */
    int32_t value; /**< co-op events (kind NCSDK_POLL_EVENT, a 9..15): their value; NCSDK_POLL_VOTE: NCSDK_VOTE_* */
} NcSdkPolled;

/* ------------------------------------------------------------------------------------------------ effects on NPCs */

/* Effects on NPCs a partner leads (sdk/docs/npc-effects.md). An NPC NightCoop syncs is led by one player (its owner,
 * usually the host); everyone else has a copy that only shows the owner's NPC. A status, a hit, a push, a slow-down,
 * a change of side or a kill done on a copy is overwritten by the owner's stream — and the owner's NPC never had it.
 * npcEffect asks the owner to do it with the game, as an action of our player (the instigator is our body there): the
 * NPC's AI reacts to us, its death is ours (kill credit, "on kill" effects), the partners see it on their copies.
 * On an NPC we lead (or one NightCoop does not sync) it is done here at once, the same way. */

/** NcSdkNpcEffect.kind */
enum
{
    NCSDK_NPC_STATUS = 1,   /**< apply the status effect `record` (StatusEffect TweakDBID) */
    NCSDK_NPC_UNSTATUS = 2, /**< remove one stack of the status effect `record` */
    NCSDK_NPC_DAMAGE = 3,   /**< `value` health points (NCSDK_NPC_PERCENT: percent of max health) as a hit of attack
                                 `record` (Attack TweakDBID; 0 — health written directly), at `point`, along `vector` */
    NCSDK_NPC_IMPULSE = 4,  /**< ragdoll and push: `vector` impulse at `point`, `value` influence radius (0 — 1 m) */
    NCSDK_NPC_SLOW = 5,     /**< its own time: `value` dilation 0..1 for `seconds` (0 — frozen, 1 — back to normal) */
    NCSDK_NPC_ATTITUDE = 6, /**< its attitude to us: `mode` 0 friendly, 1 neutral, 2 hostile (NCSDK_NPC_ALL_PLAYERS) */
    NCSDK_NPC_SIDE = 7,     /**< its side: `mode` NCSDK_NPC_SIDE_* */
    NCSDK_NPC_KILL = 8,     /**< kill it, we are the killer */
    /** its AI role: `mode` 1 — follow us (AIFollowerRole, as companion mods), 0 — no role. The role stays with the NPC when
     *  another player comes to lead it (it follows us there too) while we are in that player's world. */
    NCSDK_NPC_FOLLOW = 9,
    /** god mode (GodModeSystem): `mode` 0 — mortal again, 1 — immortal, 2 — invulnerable; stays with the NPC as FOLLOW does */
    NCSDK_NPC_GODMODE = 10,
    NCSDK_NPC_FAKE_DEATH = 11, /**< its AI plays dying without dying (ScriptedPuppet.SendAIDeathSignal) */
    NCSDK_NPC_DISPOSE = 12,    /**< remove it from the world (its copies go too) */
    NCSDK_NPC_HIDE = 13,       /**< hide it for good (HideIrreversibly), on every copy too */
};

/** NcSdkNpcEffect.flags */
enum
{
    /** STATUS / UNSTATUS: do not put it on (take it off) our own copy — the mod did that itself. Without it NightCoop does
     *  it on our copy too, from our player, as the game does for a quickhack: the status shows here and its damage ticks
     *  are our hits (they reach the owner as our damage). The owner's ticks of it write no health (that would count twice). */
    NCSDK_NPC_SKIP_LOCAL = 1u << 0,
    NCSDK_NPC_PERCENT = 1u << 1,     /**< DAMAGE: value is percent of the NPC's max health */
    NCSDK_NPC_NONLETHAL = 1u << 2,   /**< DAMAGE: it can knock down (defeat), not kill */
    NCSDK_NPC_REACT = 1u << 3,       /**< DAMAGE without a record: a hit reaction of the gamedataAttackType `mode` */
    NCSDK_NPC_ALL_PLAYERS = 1u << 4, /**< ATTITUDE: to every player of the session, not only to us */
    /** STATUS: show it on the partners' copies whatever its type (by default only the kinds that are only a look are shown:
     *  burning, short circuit, EMP, jam, poison, shock, blindness, staggers). */
    NCSDK_NPC_SHOW_ALWAYS = 1u << 5,
    NCSDK_NPC_NO_ANIM = 1u << 6,     /**< KILL: no death animation (skipNPCDeathAnim) */
    NCSDK_NPC_NO_RAGDOLL = 1u << 7,  /**< KILL: no ragdoll (disableNPCRagdoll) */
};

/** NCSDK_NPC_SIDE modes */
enum
{
    NCSDK_NPC_SIDE_JOIN = 0,        /**< our side: our attitude group, friendly to us (as the game's ally quickhacks) */
    NCSDK_NPC_SIDE_ALL_ENEMIES = 1, /**< hostile to everyone (the group HostileToEveryone, as the game's madness) */
    NCSDK_NPC_SIDE_OWN = 2,         /**< back to its own group (its character record's base attitude group) */
};

typedef struct NcSdkNpcEffect
{
    uint32_t size;    /**< sizeof(NcSdkNpcEffect) */
    int32_t kind;     /**< NCSDK_NPC_* */
    uint32_t flags;   /**< NCSDK_NPC_SKIP_LOCAL, ... */
    int32_t mode;     /**< ATTITUDE: EAIAttitude; SIDE: NCSDK_NPC_SIDE_*; DAMAGE with NCSDK_NPC_REACT: attack type;
                           FOLLOW: 1 follow / 0 stop; GODMODE: 0 mortal, 1 immortal, 2 invulnerable */
    uint64_t record;  /**< TweakDBID: a status (STATUS, UNSTATUS) or an attack (DAMAGE) */
    float value;      /**< DAMAGE: health points or percent; IMPULSE: influence radius; SLOW: dilation (0 — frozen) */
    float seconds;    /**< SLOW: how long, 0 < seconds <= NCSDK_NPC_SLOW_MAX_S */
    NcSdkVec3 point;  /**< world space, now: DAMAGE — where it hits (0,0,0 — the chest); IMPULSE — where it pushes */
    NcSdkVec3 vector; /**< world space: DAMAGE — the hit direction (0 — from us); IMPULSE — the impulse */
} NcSdkNpcEffect;

#define NCSDK_NPC_DATA_MAX 1024     /**< npcRequest / npcReply payload, bytes */
#define NCSDK_NPC_RATE_PER_S 20     /**< requests per mod per second (bursts up to 2x); the owner — per mod and partner */
#define NCSDK_NPC_PENDING_MAX 256   /**< unanswered npcRequest per mod at the owner; older ones answer NCSDK_E_LIMIT */
#define NCSDK_NPC_NEAR_M 150        /**< the owner does a request only if our body is this near its NPC */
#define NCSDK_NPC_SLOW_MAX_S 120    /**< longest SLOW */
#define NCSDK_NPC_IMPULSE_MAX 1000  /**< the longest IMPULSE vector */
#define NCSDK_WORLD_SLOW_MAX_S 120  /**< the longest worldSlow */

/* ------------------------------------------------------------------------------------- the save, the menu, votes, partners */
/* The guest's entry into the host's world by save, mods' items in the co-op menu, votes of mods, what a partner is doing
 * and co-op events. */

/** What a mod uses (passport flags, as NCSDK_MOD_USES_*: an undeclared use answers NCSDK_E_DENIED, sdk/SECURITY.md). */
enum
{
    NCSDK_MOD_USES_SAVE = 1u << 15,  /**< scriptableScope */
    NCSDK_MOD_USES_MENU = 1u << 16,  /**< menuItemSet, menuItemRemove */
    NCSDK_MOD_USES_VOTES = 1u << 17, /**< voteStart, voteCancel; the host's votes of the mod on a guest */
    NCSDK_MOD_USES_WORLD_ASK = 1u << 23, /**< worldAsk; a guest's asks of the mod at the host */
};

/** Co-op events: hooks.coopEvent (with value, entity, ref) or hooks.event (event and peer), script event kind 1 with
 *  a = the event and `value`. What a value says now, partnerState and where also say. Partners — in our world only. */
enum
{
    /** A partner fell (value 1) or got up (value 0): revived, their own second heart, came to after passing out. */
    NCSDK_EV_PEER_DOWN = 9,
    /** Every player of the host's world is down — the co-op death (no one dies alone in a session): value 1 — the host
     *  loads its last save, everyone wakes up in it; 0 — there is no save, everyone comes to in a safe place. Peer NONE. */
    NCSDK_EV_TEAM_DOWN = 10,
    /** A partner got into a vehicle (value 1 a passenger, 2 at the wheel) or out of it (0); ref and entity — the vehicle
     *  (ref kind NCSDK_NET_NONE when NightCoop does not share that vehicle; entity 0 when it is not here). */
    NCSDK_EV_PEER_VEHICLE = 11,
    /** An NPC NightCoop syncs was killed by a player: peer — the killer (may be our own number), ref — the NPC, entity —
     *  our copy or our own NPC (0 when it is not here). NPCs nobody shares (purely local) give no event. */
    NCSDK_EV_NPC_KILLED = 12,
    /** Every player of the host's world is in a menu: the world stands (value 1) / runs again (0). Peer NONE. */
    NCSDK_EV_WORLD_PAUSE = 13,
    /** The host saved (value 1) or loaded a save (value 2). Peer — the host. */
    NCSDK_EV_HOST_SAVE = 14,
    /** The host skipped time (bed, waiting, a scene); the guests' players are fast-forwarded as the game does it for the
     *  one who skipped. Value — the world's game time after the skip, seconds. Peer — the host. */
    NCSDK_EV_TIME_SKIP = 15,
    /** The answer to a worldAsk of this mod — only to the mod that asked (on the asker's machine) and to its copy at the host:
     *  peer — who asked, value — NCSDK_WORLD_ANSWER_*, entity — the number worldAsk returned (not an entity). */
    NCSDK_EV_WORLD_ANSWER = 32,
};

/** worldAsk kinds: what a mod may ask the host to change in the world. */
enum
{
    NCSDK_WORLD_TIME_OF_DAY = 1, /**< value — seconds of the day 0..86399: the world goes forward to that time (at most a day) */
    NCSDK_WORLD_WEATHER = 2,     /**< text — a weather state ([a-z0-9_], e.g. 24h_weather_rain), value — blend seconds 0..600 */
};

/** worldAsk flags. */
enum
{
    NCSDK_WORLD_ASK_VOTE = 1u << 0, /**< the host decides by a vote of every player of its world, not by its copy of the mod */
};

/** NCSDK_EV_WORLD_ANSWER value. */
enum
{
    NCSDK_WORLD_ANSWER_REFUSED = 0, /**< the host's mod or the vote said no; or the asking mod is not allowed */
    NCSDK_WORLD_ANSWER_DONE = 1,    /**< the host changed it; partners get it as the host's time and weather */
    NCSDK_WORLD_ANSWER_FAILED = 2,  /**< agreed, but the game did not take it (no such weather state; no world), or the
                                         session ended before the answer */
    NCSDK_WORLD_ANSWER_BUSY = 3,    /**< not now: another vote runs, or this player asks too often */
};

/** More kinds of NcSdkPolled.kind. */
enum
{
    NCSDK_POLL_MENU = 11, /**< key — the item key of this mod the player pressed in the co-op menu; peer — we */
    NCSDK_POLL_VOTE = 12, /**< key — the vote key; a — the winning option (0-based, -1 none); value — NCSDK_VOTE_*;
                               data — the votes per option, one byte each, and last one byte: how many could vote
                               (dataSize — the number of options + 1) */
};

typedef struct NcSdkCoopEvent
{
    uint32_t size;    /**< sizeof(NcSdkCoopEvent) of NightCoop */
    int32_t event;    /**< NCSDK_EV_PEER_DOWN .. NCSDK_EV_TIME_SKIP */
    NcSdkPeer peer;
    int32_t value;
    uint64_t entity;  /**< our local entity (EntityID hash) it is about, 0 — none here */
    NcSdkNetRef ref;  /**< its network reference, kind NCSDK_NET_NONE — none */
} NcSdkCoopEvent;

/** scriptableScope: where the guest's game takes a script system (ScriptableSystem) of the mod from when the guest
 *  enters the host's world by the host's save (NightCoop builds the guest's save from the host's world and the guest's
 *  character). */
enum
{
    NCSDK_SCOPE_DEFAULT = 0,   /**< as NightCoop decides: a mod's system is the character's (each player's own) */
    NCSDK_SCOPE_WORLD = 1,     /**< the world's: the guest gets the host's state of the system (back home — their own) */
    NCSDK_SCOPE_CHARACTER = 2, /**< each player's own (the default, said aloud) */
};

/** partnerState flags. */
enum
{
    NCSDK_PARTNER_IN_WORLD = 1u << 0,  /**< in our world (both in the host's world); the flags below are fresh only with it */
    NCSDK_PARTNER_DOWN = 1u << 1,      /**< lies downed */
    NCSDK_PARTNER_COMBAT = 1u << 2,    /**< the game says they are in combat */
    NCSDK_PARTNER_VEHICLE = 1u << 3,   /**< in a vehicle (getting in and out too) */
    NCSDK_PARTNER_WHEEL = 1u << 4,     /**< at the wheel of a vehicle NightCoop shares */
    NCSDK_PARTNER_SCENE = 1u << 5,     /**< a scene leads them (a dialogue, a workspot, a cutscene: scene tier 2..5) */
    NCSDK_PARTNER_MENU = 1u << 6,      /**< their pause menu or a full-screen game window is open */
    NCSDK_PARTNER_LINK_LOST = 1u << 7, /**< the link to them is lost for now (the session waits for them) */
    NCSDK_PARTNER_SELF = 1u << 8,      /**< it is us (partnerState of our own number) */
};

/** partnerState where. */
enum
{
    NCSDK_WHERE_UNKNOWN = 0,
    NCSDK_WHERE_HOST_WORLD = 1, /**< in the host's world */
    NCSDK_WHERE_MAIN_MENU = 2,
    NCSDK_WHERE_LOADING = 3,
    NCSDK_WHERE_JOINING = 4,    /**< in their own world, entering the host's (waits for its save, builds it, loads) */
    NCSDK_WHERE_OWN_WORLD = 5,  /**< in their own world */
};

typedef struct NcSdkPartnerState
{
    uint32_t size;          /**< set by the caller: sizeof(NcSdkPartnerState) */
    uint32_t flags;         /**< NCSDK_PARTNER_* */
    int32_t where;          /**< NCSDK_WHERE_* */
    float health;           /**< 0..1 of their full health; -1 — not known */
    int32_t level;          /**< their character level; 0 — not known (they did not tell it yet) */
    uint32_t reserved;
    NcSdkNetRef vehicle;    /**< the shared vehicle they sit in (kind NCSDK_NET_VEHICLE), else kind NCSDK_NET_NONE */
    uint64_t vehicleEntity; /**< our entity of that vehicle (our mirror or our own), 0 — not here */
} NcSdkPartnerState;

/** menuItemSet flags. */
enum
{
    NCSDK_MENU_DISABLED = 1u << 0,   /**< shown, but not now: a press says so and does not reach the mod */
    NCSDK_MENU_IN_SESSION = 1u << 1, /**< only in a session */
    NCSDK_MENU_HOST_ONLY = 1u << 2,  /**< not for guests (the host and out of session) */
    NCSDK_MENU_IN_WORLD = 1u << 3,   /**< only in the pause menu (in a world), not in the main menu */
};

/** How a vote ended (NcSdkVoteResult.how, the value of NCSDK_POLL_VOTE). */
enum
{
    NCSDK_VOTE_DECIDED = 1,   /**< the votes reached what the host's voting mode needs (everyone / more than half) */
    NCSDK_VOTE_FORCED = 2,    /**< the host applied its choice (held End, as for NightCoop's own choices) */
    NCSDK_VOTE_TIMED_OUT = 3, /**< NightCoop's voting time ran out (when the host has one): the host's vote, else the most */
    NCSDK_VOTE_CANCELLED = 4, /**< the mod cancelled it (voteCancel) */
    NCSDK_VOTE_ABORTED = 5,   /**< no vote is possible any more: the session ended, the host left its world, switched off */
};

typedef struct NcSdkVoteResult
{
    uint32_t size;      /**< sizeof(NcSdkVoteResult) of NightCoop */
    int32_t how;        /**< NCSDK_VOTE_* */
    int32_t winner;     /**< the decided option, 0-based; -1 — none (cancelled, aborted) */
    uint32_t options;   /**< how many options the vote had */
    uint32_t voters;    /**< how many players could vote */
    uint8_t votes[8];   /**< votes per option (NCSDK_VOTE_OPTIONS_MAX) */
    const char* key;    /**< the vote key of voteStart */
} NcSdkVoteResult;

/* ------------------------------------------------------------------------------------------------ the table */

typedef struct NcSdkApi
{
    uint32_t size;      /**< sizeof(NcSdkApi) of this NightCoop */
    uint32_t version;   /**< NCSDK_VERSION */
    uint32_t protocol;  /**< NightCoop's own wire version (informational) */
    uint32_t reserved;
    const char* build;  /**< NightCoop build stamp (commit) */

    /* --- mods */
    NcSdkMod (*registerMod)(const NcSdkModInfo* aInfo);
    void (*unregisterMod)(NcSdkMod aMod);

    /* --- log, screen, language (any thread) */
    void (*log)(NcSdkMod aMod, int32_t aLevel, const char* aText);
    /** A one-off message at the bottom of the screen (NightCoop's notes, the game's language font). */
    int32_t (*showMessage)(NcSdkMod aMod, const char* aText, float aSeconds, int32_t aTone);
    /** A label in the game's UI language: aRu for Russian, aEn for English, NightCoop's translation of aEn (if it has
     *  one) for the other 17 languages, else aEn. The pointer lives while NightCoop is loaded. */
    const char* (*pick)(const char* aRu, const char* aEn);

    /* --- session */
    void (*where)(NcSdkWhere* aOut);
    /** Other players of the session. Returns how many there are (fills at most aMax). */
    size_t (*peers)(NcSdkPeer* aOut, size_t aMax);
    /** The player's name as in the player list. Returns its length (aOut gets it cut, zero-terminated). */
    size_t (*peerName)(NcSdkPeer aPeer, char* aOut, size_t aCap);
    /** Both of us are in the host's world. */
    int32_t (*peerInOurWorld)(NcSdkPeer aPeer);
    /** 1 — the peer registered this mod (same id and protocol); 0 — not (or another protocol); NCSDK_E_STATE — not
     *  known yet (the mods check with them is not over). */
    int32_t (*peerHasMod)(NcSdkMod aMod, NcSdkPeer aPeer);
    /** The peer's version string of this mod. Returns its length or a negative NCSDK_E_*. */
    int32_t (*peerModVersion)(NcSdkMod aMod, NcSdkPeer aPeer, char* aOut, size_t aCap);

    /* --- channels: messages of this mod to peers that have it */
    /** aTo — a peer, NCSDK_PEER_HOST or NCSDK_TO_ALL; aChannel — the mod's own numbering; aFlags — NCSDK_SEND_*.
     *  Guests' messages to other guests are relayed by the host. Nothing is sent to peers without the mod. */
    int32_t (*send)(NcSdkMod aMod, NcSdkPeer aTo, uint16_t aChannel, const void* aData, size_t aSize, uint32_t aFlags);

    /* --- shared state of this mod: key -> value, kept by the host, the same for everyone in the session */
    /** Host: set and spread. Guest: ask the host (hooks.stateRequest there); the value changes when the host's answer
     *  comes (stateChanged). aSize 0 — remove the key. */
    int32_t (*stateSet)(NcSdkMod aMod, const char* aKey, const void* aData, size_t aSize);
    /** Copy the current value. Returns its size (NCSDK_E_SIZE if aCap is smaller, NCSDK_E_UNKNOWN if no such key). */
    int32_t (*stateGet)(NcSdkMod aMod, const char* aKey, void* aOut, size_t aCap);

    /* --- entities */
    /** Local entity (EntityID hash) -> network reference. NCSDK_E_UNKNOWN — NightCoop does not sync it. */
    int32_t (*entityRef)(uint64_t aEntity, NcSdkNetRef* aOut);
    /** Network reference -> our local entity (mirror, copy or our own). 0 — not here (not streamed, not spawned). */
    uint64_t (*entityOfRef)(const NcSdkNetRef* aRef);
    /** 1 — this machine leads the entity; 0 — a partner does (ours is a mirror or copy); NCSDK_E_UNKNOWN — not synced
     *  by NightCoop (then it is purely local: yours). */
    int32_t (*entityIsMine)(uint64_t aEntity);
    /** The body of the partner aPeer in our world. 0 — none now. */
    uint64_t (*partnerBody)(NcSdkPeer aPeer);
    /** Owner only: attach a value of this mod under aKey to the entity; partners get it on their mirror/copy
     *  (hooks.entityState), late joiners too while the entity lives. aFlags — NCSDK_STATE_STREAM. */
    int32_t (*entitySetState)(NcSdkMod aMod, uint64_t aEntity, uint16_t aKey, const void* aData, size_t aSize,
                              uint32_t aFlags);
    int32_t (*entityClearState)(NcSdkMod aMod, uint64_t aEntity, uint16_t aKey);
    /** Share an entity this mod spawned: partners get a copy (template, appearance, transform), the owner leads it.
     *  aFlags — NCSDK_SHARE_*. Returns NCSDK_OK; the reference — entityRef. */
    int32_t (*entityShare)(NcSdkMod aMod, uint64_t aEntity, uint32_t aFlags);
    int32_t (*entityUnshare)(NcSdkMod aMod, uint64_t aEntity);
    /** Keep the entity purely local: NightCoop never adopts, leads, hides or removes it (population, NPC copies,
     *  crowd rules). aOn 0 — back to normal. */
    int32_t (*entityKeepLocal)(NcSdkMod aMod, uint64_t aEntity, int32_t aOn);

    /* --- pose source of our own player */
    int32_t (*poseSet)(NcSdkMod aMod, const NcSdkPose* aPose);
    void (*poseClear)(NcSdkMod aMod);
    /** The last pose of a partner's source (NCSDK_E_UNKNOWN — they have none). */
    int32_t (*partnerPose)(NcSdkPeer aPeer, NcSdkPose* aOut);

    /* --- NightCoop's own actions by name: what its keys and menu items do, for VR controllers, gamepads, overlays */
    /** Names, '\n'-separated. Returns the full length (aOut gets it cut, zero-terminated). Known names:
     *  "coop_menu", "story_pull", "story_without_partner", "revive_partner", "save_pull", "vote_yes", "vote_no",
     *  "pass_out" (vote_no withdraws our own vote; coop_menu works only while the pause or main menu is open). */
    size_t (*actionList)(char* aOut, size_t aCap);
    /** 1 — can be done now, 0 — not now, NCSDK_E_UNKNOWN — no such action. */
    int32_t (*actionAvailable)(const char* aName);
    /** Do it as if its key were pressed. NCSDK_OK, NCSDK_E_STATE (not now), NCSDK_E_UNKNOWN. */
    int32_t (*actionInvoke)(NcSdkMod aMod, const char* aName);

    /* --- events for mods without hooks (scripts, CET) */
    /** Take the next queued item of this mod (out->kind 0 — none). The queue is kept only for mods registered without
     *  hooks.message/entityState/...: a mod with hooks gets them by callback. Max 1024 items, the oldest drop. */
    int32_t (*pollEvent)(NcSdkMod aMod, NcSdkPolled* aOut);

    /* --- per-player quest facts */
    /** Mark a quest fact (its name as in QuestsSystem.SetFact and quest phases) as each player's own while this mod is
     *  registered. NightCoop mirrors the host's quest facts to the guests (the host's world decides quests); a fact a mod
     *  keeps for its own player — its scenes on the local V, its settings, its per-character state — must not travel:
     *  with this mark the host does not send its value, a guest does not take the host's value, and a quest pause that
     *  waits for the fact on a guest goes by the guest's own value instead of waiting for the host's progress.
     *  aOn 0 — the fact is the world's (shared) again. Unregistering the mod drops all its marks.
     *  Returns NCSDK_OK; NCSDK_E_ARG (bad handle or name: NCSDK_FACT_NAME_MAX bytes of printable ASCII),
     *  NCSDK_E_LIMIT (NCSDK_LOCAL_FACTS_MAX per mod), NCSDK_E_OFF (the player switched "sdk_local_facts" off).
     *  Any thread. Mark the facts before the session starts (right after registerMod) so the first snapshot skips them.
     *  The guest's entry into the host's world by save keeps the guest's own value too
     *  (switch "sdk_facts_save"). */
    int32_t (*factKeepLocal)(NcSdkMod aMod, const char* aFact, int32_t aOn);

    /* --- vehicles a mod holds in the air */
    /** The mod holds this vehicle in the air on THIS machine: a flight mod flies our own vehicle (its flight, its gravity
     *  switched off). While the hold is on, NightCoop's rules that assume a car stands on the ground leave the vehicle
     *  alone: a mirror is not let into the game's physics when a car touches it (a flown car would just drop, as an AV
     *  would), our own vehicle is not returned home to a partner who does not have this mod (it would fall there). The
     *  hold of a vehicle we lead is told to every partner, with or without the mod: their mirror of it goes by the same
     *  rules, a partner without the mod does not take its wheel (it flies on at its owner), and with
     *  NCSDK_AIRBORNE_NO_WHEELS a partner without the mod sees it without wheels. aOn — NCSDK_AIRBORNE_* bits (any other
     *  non-zero value is NCSDK_AIRBORNE_HOLD), 0 — release. Holds are dropped with unregisterMod; release them when the
     *  flight ends or the entity goes away. Returns NCSDK_OK, NCSDK_E_ARG (bad handle or entity), NCSDK_E_OFF (the
     *  player switched it off: "port_flight_air_hold"). */
    int32_t (*entityAirborne)(NcSdkMod aMod, uint64_t aEntity, int32_t aOn);

    /* --- text from partners */
    /** Make a text safe to show or log: valid UTF-8 (bad sequences become U+FFFD), no control characters (C0, DEL, C1),
     *  no invisible or direction-changing characters (bidi marks and overrides, zero-width, tags, fillers), tabs and line
     *  breaks become spaces, runs of spaces collapse, at most 2 combining marks in a row, at most aMaxBytes bytes
     *  (0 — NCSDK_TEXT_MAX; never cuts a character). The same cleaning NightCoop applies to everything a partner sends that
     *  ends on the screen through the SDK (passport names and versions, showMessage, log). Use it on every text a partner's
     *  copy of your mod sends you before you show it. Returns the cleaned length; aOut gets it cut to aCap - 1,
     *  zero-terminated. Any thread. */
    size_t (*cleanText)(const char* aText, size_t aMaxBytes, char* aOut, size_t aCap);

    /* --- effects on NPCs a partner leads (sdk/docs/npc-effects.md). Need NCSDK_MOD_USES_NPC_REQUESTS. */
    /** Do aEffect on the NPC aEntity (our local entity: a copy of a partner's NPC, or ours). Ours (or not synced by
     *  NightCoop) — done here at once with the game, returns NCSDK_OK. A partner's — a request to its owner, returns its
     *  number (> 0); the owner does it with the game from our body and answers (hooks.npcResult / NCSDK_POLL_NPC_RESULT):
     *  NCSDK_OK, NCSDK_E_UNKNOWN (the record is not in their game, the NPC is gone), NCSDK_E_DENIED (they lead it no more —
     *  ask again through entityRef — or they refused), NCSDK_E_STATE (our body is not near it, the NPC cannot do it now),
     *  NCSDK_E_LIMIT. Refused at once: NCSDK_E_ARG (bad effect), NCSDK_E_UNSUPPORTED (unknown kind), NCSDK_E_UNKNOWN (no such
     *  entity, the record is not in our game), NCSDK_E_STATE (not in the same world with the owner, the owner's passport is
     *  not known yet), NCSDK_E_DENIED (no permission, the owner has not this mod with this protocol), NCSDK_E_LIMIT
     *  (NCSDK_NPC_RATE_PER_S), NCSDK_E_OFF ("sdk_npc"). */
    int32_t (*npcEffect)(NcSdkMod aMod, uint64_t aEntity, const NcSdkNpcEffect* aEffect);
    /** The general request: run this mod's own logic on the machine that leads the NPC aEntity. Its owner's copy of the mod
     *  gets hooks.npcRequest / NCSDK_POLL_NPC_REQUEST with its local NPC (and aChannel, aData — yours), does what it likes
     *  with the game and answers with npcReply; we get the answer as hooks.npcResult (aEffect 0). Always returns the
     *  request number (> 0) — also for our own NPC: then the request comes to this very mod. aSize <= NCSDK_NPC_DATA_MAX.
     *  Refusals as npcEffect. */
    int32_t (*npcRequest)(NcSdkMod aMod, uint64_t aEntity, uint16_t aChannel, const void* aData, size_t aSize);
    /** The owner's answer to the request aRequest of hooks.npcRequest: aResult NCSDK_OK or a negative NCSDK_E_*, aData the
     *  reply (<= NCSDK_NPC_DATA_MAX). Answer each request once. Returns NCSDK_OK, NCSDK_E_UNKNOWN (no such open request: it
     *  was answered, or dropped as the oldest of NCSDK_NPC_PENDING_MAX, or its asker left), NCSDK_E_ARG. */
    int32_t (*npcReply)(NcSdkMod aMod, uint32_t aRequest, int32_t aResult, const void* aData, size_t aSize);
    /** This mod slows (or stops) OUR world now, aDilation 0..1 for aSeconds (<= NCSDK_WORLD_SLOW_MAX_S; aDilation 1 — it is
     *  over): bullet time, a stasis field, a time-stop implant. Declare it, then slow the world with the game yourself
     *  (TimeSystem.SetTimeDilation with your own reason, or TimeDilationHelper). NightCoop then: gives the partners near us
     *  the same slow-down as from a Sandevistan (their world within the implant radius, our body at our pace there); does not
     *  take a world stopped this way for a loading screen, here or at those partners; lets TimeDilationHelper profiles
     *  through in co-op while it is declared. Needs NCSDK_MOD_USES_WORLD_TIME. Returns NCSDK_OK, NCSDK_E_ARG, NCSDK_E_DENIED,
     *  NCSDK_E_OFF ("sdk_world_slow"). Check with NCSDK_HAS(api, worldSlow). */
    int32_t (*worldSlow)(NcSdkMod aMod, float aDilation, float aSeconds);
    /* --- the save, the co-op menu, votes, partners */
    /** When a guest enters the host's world by the host's save, NightCoop builds the guest's save from the host's world and
     *  the guest's character: quest facts, the journal and the world's script systems (fast travel, police, …) come from
     *  the host, the character's systems from the guest. A ScriptableSystem of a mod is the character's by default.
     *  NCSDK_SCOPE_WORLD — the guest takes the host's state of it (a system that keeps the world of the mod: its NPCs, its
     *  places, its world events); back in the guest's own campaign it is the campaign's own again. NCSDK_SCOPE_CHARACTER —
     *  each player's own (as the default). NCSDK_SCOPE_DEFAULT — drop the word. aClass — the class name as the game's RTTI
     *  has it (redscript: NameToString(this.GetClassName()) — "Module.Class" for a class in a module); it must be a
     *  ScriptableSystem, not one of the game's systems NightCoop already takes from the world. Facts marked with
     *  factKeepLocal keep the guest's own value at that entry too (and at the way back home — the value of the session).
     *  Say it right after registerMod: it counts at the moment the guest enters. Unregistering drops the mod's words.
     *  NCSDK_OK; NCSDK_E_ARG (bad handle, name or scope), NCSDK_E_UNKNOWN (no such class, or not a ScriptableSystem),
     *  NCSDK_E_DENIED (not declared NCSDK_MOD_USES_SAVE, a game system NightCoop decides, or another mod said otherwise),
     *  NCSDK_E_LIMIT (NCSDK_SCOPES_MAX per mod), NCSDK_E_OFF ("sdk_save_scopes" off). Game thread. */
    int32_t (*scriptableScope)(NcSdkMod aMod, const char* aClass, int32_t aScope);
    /** Add or change an item of this mod in NightCoop's co-op menu (Esc → «Co-op» → «Mods…»; in the main menu too).
     *  aKey — the item's own key ([a-z0-9._-], 1..NCSDK_MENU_KEY_MAX); aRu / aEn — its label (pick() rules: the player's
     *  language), at most NCSDK_MENU_LABEL_MAX bytes; aFlags — NCSDK_MENU_*. A press comes as hooks.menuItem or
     *  NCSDK_POLL_MENU with the key. NCSDK_OK; NCSDK_E_ARG, NCSDK_E_DENIED (not declared NCSDK_MOD_USES_MENU), NCSDK_E_LIMIT
     *  (NCSDK_MENU_ITEMS_MAX per mod, 32 in all), NCSDK_E_OFF ("sdk_menu" off). Any thread. */
    int32_t (*menuItemSet)(NcSdkMod aMod, const char* aKey, const char* aRu, const char* aEn, uint32_t aFlags);
    /** Remove the item. NCSDK_OK, NCSDK_E_UNKNOWN (no such item). Unregistering removes all items of the mod. Any thread. */
    int32_t (*menuItemRemove)(NcSdkMod aMod, const char* aKey);
    /** Host only: ask every player of the host's world a question in NightCoop's voting window (the dialogue choice
     *  window: arrows, F or Enter, NumPad 1..8; who voted for what is written on the options). The host's voting mode
     *  decides (everyone / more than half, «Extra settings» of the co-op menu); the host can apply its choice by holding
     *  End, as for NightCoop's choices. aQuestionRu/En, aOptionsRu/En[aCount] — the texts in Russian and English (each
     *  player sees their own language, pick() rules; never the host's text in another language), 2..NCSDK_VOTE_OPTIONS_MAX
     *  options. The window shows the question when no choice of the game is on screen; one vote at a time in a session.
     *  The end comes to the mod on every machine that has it: hooks.voteResult or NCSDK_POLL_VOTE, with aKey. A guest's
     *  copy of the mod asks the host's copy by a channel message. NCSDK_OK; NCSDK_E_ARG, NCSDK_E_STATE (no session, not
     *  in the world, another vote runs), NCSDK_E_DENIED (a guest, not declared NCSDK_MOD_USES_VOTES, the host switched
     *  mods off for the session), NCSDK_E_OFF ("sdk_votes" off). Game thread. */
    int32_t (*voteStart)(NcSdkMod aMod, const char* aKey, const char* aQuestionRu, const char* aQuestionEn,
                         const char* const* aOptionsRu, const char* const* aOptionsEn, uint32_t aCount);
    /** Host only: stop the mod's vote aKey (NCSDK_VOTE_CANCELLED everywhere). NCSDK_OK, NCSDK_E_UNKNOWN. Game thread. */
    int32_t (*voteCancel)(NcSdkMod aMod, const char* aKey);
    /** What a player is doing (a partner, or ourselves — NCSDK_PARTNER_SELF): in our world, downed, in combat, in a
     *  vehicle (and which), in a scene, in a menu, health and level. Reading — no flag needed. NCSDK_OK; NCSDK_E_ARG
     *  (NULL or a struct too short), NCSDK_E_STATE (no session), NCSDK_E_UNKNOWN (no such player). Game thread. */
    int32_t (*partnerState)(NcSdkPeer aPeer, NcSdkPartnerState* aOut);
    /** factKeepLocal for a list of facts at once (a mod with many, a hundred and more). Returns how many were accepted
     *  (>= 0; the refused ones are in the log), or NCSDK_E_ARG / NCSDK_E_DENIED / NCSDK_E_OFF for the call. Any thread.
     *  A mod without code says the same in its manifest (sdk/docs/manifest.md). */
    int32_t (*factKeepLocalList)(NcSdkMod aMod, const char* const* aFacts, uint32_t aCount, int32_t aOn);
    /** Ask the host to change the world: the host leads time and weather, a guest's own change is undone in seconds.
     *  aKind — NCSDK_WORLD_TIME_OF_DAY or NCSDK_WORLD_WEATHER (only these: the game has no setter of the pace of time for
     *  scripts, and the world's pause belongs to NightCoop's menu); aValue, aText — see the kind. The host decides: the hook
     *  worldRequest of its copy of the mod, or — with NCSDK_WORLD_ASK_VOTE, or when the host's mod has no such hook or is
     *  not there — a vote of every player of the host's world in NightCoop's choice window. The host sets it by the game's
     *  own way; partners get it as the host's time (GameClock) and weather (weather_host). The answer —
     *  NCSDK_EV_WORLD_ANSWER to this mod (here and at the host). On the host the ask is done at once (or voted with the
     *  flag). Returns the ask's number (> 0) or NCSDK_E_ARG, NCSDK_E_STATE (no session, not in the host's world),
     *  NCSDK_E_DENIED (not declared NCSDK_MOD_USES_WORLD_ASK; mods off for the session), NCSDK_E_LIMIT (asks too often:
     *  one in 2 seconds, two in a row), NCSDK_E_OFF ("sdk_world_ask" off). Game thread. */
    int32_t (*worldAsk)(NcSdkMod aMod, int32_t aKind, int32_t aValue, const char* aText, uint32_t aFlags);
    /* --- shared spawns: who leads a shared spawn, NCSDK_SHARE_HANDOFF, mod vehicles as NightCoop's shared vehicles;
     * effects shown to partners (sdk/docs/shared-spawns.md). */
    /** Who leads the entity now: the peer whose machine runs its AI or physics (a shared NPC handed to the nearest player,
     *  a shared vehicle — its driver), NightCoop's synced NPCs and vehicles too. NCSDK_PEER_NONE — not synced by NightCoop
     *  (then it is purely local: yours), or not in the world. Reading: no permission needed. Game thread. */
    NcSdkPeer (*entityLeader)(uint64_t aEntity);
    /** Show a world effect (an .effect resource: its picture and sound, no damage, no logic) on the machines of the partners
     *  in our world, fire and forget — the way NightCoop shows the effects of the players' attacks. Your own machine: play
     *  it yourself (FxSystem.SpawnEffect). aPath — the resource path as in the game's archives, with backslashes (printable ASCII,
     *  at most NCSDK_FX_PATH_MAX bytes, ending in ".effect"); a partner without that resource shows nothing. aAt — world
     *  space, within NightCoop's sight of your body (a partner drops an effect farther than 150 m from your player). aFlags —
     *  NCSDK_FX_*. Needs NCSDK_MOD_USES_EFFECTS. Returns how many partners in our world it went to (0 — nobody),
     *  NCSDK_E_ARG (bad path, transform or flag), NCSDK_E_STATE (not in the world, no session), NCSDK_E_LIMIT (over
     *  NCSDK_FX_PER_S), NCSDK_E_OFF (switch "sdk_fx"), NCSDK_E_DENIED (not declared, or mods off for this session).
     *  Game thread. */
    int32_t (*effectShow)(NcSdkMod aMod, const char* aPath, const NcSdkTransform* aAt, uint32_t aFlags);
    /** Attach an effect to an entity NightCoop syncs (a player, a vehicle, an NPC, a shared spawn) on the partners' machines
     *  in our world: on each it goes onto their mirror or copy of that entity. aEffect — an .effect resource path, attached
     *  to the slot aSlot of the entity (NULL or "" — its root); with NCSDK_FX_BY_NAME — the name of an effect of the
     *  entity's own template (GameObjectEffectHelper.StartEffectEvent). Play it on your own entity yourself. *aOutEffect —
     *  the number for effectStop. Needs NCSDK_MOD_USES_EFFECTS. Returns how many partners got it, or NCSDK_E_UNKNOWN (the
     *  entity is not synced), NCSDK_E_ARG, NCSDK_E_STATE, NCSDK_E_LIMIT, NCSDK_E_OFF. A partner without the resource or
     *  without that effect name shows nothing. Game thread. */
    int32_t (*effectAttach)(NcSdkMod aMod, uint64_t aEntity, const char* aEffect, const char* aSlot, uint32_t aFlags,
                            uint32_t* aOutEffect);
    /** Stop an effect of effectAttach on the partners: aBreakLoop 1 — let it end its loop (BreakLoop), 0 — at once (Kill).
     *  Returns how many partners got it, NCSDK_E_UNKNOWN (not our effect number). Game thread. */
    int32_t (*effectStop)(NcSdkMod aMod, uint32_t aEffect, int32_t aBreakLoop);
    /** Play (aStop 0) or stop (1) a sound event (an audio event name, as GameObject.PlaySound) on an entity NightCoop
     *  syncs, on the partners' mirror or copy of it. Play it on your own entity yourself. An unknown event is silent there.
     *  Needs NCSDK_MOD_USES_EFFECTS. Returns how many partners got it, or NCSDK_E_*. Game thread. */
    int32_t (*soundPlay)(NcSdkMod aMod, uint64_t aEntity, const char* aEvent, int32_t aStop);
    /** Give a reward to a partner (aTo — a peer, or NCSDK_TO_ALL: every partner in our world): aQuantity of the item record
     *  aRecord (its TweakDB name, e.g. "Items.money") is created in that player's own inventory — exactly once, a lost
     *  answer is repeated, never given twice. Nothing is taken from yours (NightCoop's item transfer does that). The
     *  partner's NightCoop checks: it has your mod too (same id and protocol), your passport declares
     *  NCSDK_MOD_USES_REWARDS, the record is an item there and not a quest item, 1..NCSDK_REWARD_MAX, the rate, one world,
     *  and — from a guest — that your player is within 100 m (the host's world gives quest and gig rewards from anywhere).
     *  The answer comes as hooks.reward / NCSDK_POLL_REWARD (peer, entity = the grant number, a = result). Returns how many
     *  grants started (*aOutGrant — the number of the first), 0 — nobody in our world, or NCSDK_E_*. Game thread. */
    int32_t (*rewardGrant)(NcSdkMod aMod, NcSdkPeer aTo, const char* aRecord, int32_t aQuantity, uint64_t* aOutGrant);
    /** Ask to lead an NPC for a while (possession, mind control, a puppet the player drives): aEntity — our body of an NPC
     *  NightCoop syncs (a partner's NPC's copy here, or one we lead). Its owner hands it over by NightCoop's usual handoff, and
     *  this machine keeps leading it for aSeconds (1..NCSDK_LEASE_MAX_S; NCSDK_LEASE_UNTIL_RELEASE — until you call again
     *  with 0): NightCoop's own handoff rules (distance, companions, scenes, a partner's grapple) do not take it away. aSeconds
     *  0 — release now: the usual rules lead it again (it goes back to the nearest player). You learn the outcome by
     *  hooks.spawnLeader / NCSDK_POLL_SPAWN_LEADER for this NPC (ref kind NCSDK_NET_NPC; aMine 1 — it is ours now, 0 — the
     *  owner did not hand it over within 5 s). Only on your request — nothing changes for other NPCs. Needs
     *  NCSDK_MOD_USES_NPC_REQUESTS. Returns NCSDK_OK (asked, or held when it is ours already), NCSDK_E_UNKNOWN (not a synced
     *  NPC / no lease to release), NCSDK_E_ARG, NCSDK_E_STATE, NCSDK_E_OFF (switch "sdk_npc_lease"). Game thread. */
    int32_t (*npcLease)(NcSdkMod aMod, uint64_t aEntity, uint32_t aSeconds);
    /* --- functions co-op relies on and NightCoop's keys (sdk/docs/do-not-hook.md) */
    /** Tell NightCoop the mod hooks a game function: a script function by aClass (script or native class name,
     *  "PlayerPuppet", "gameObject") and aFunction (short name "OnDeath", or "OnDeath;Sig"), or a native function by the
     *  address of its entry aAddress (aClass and aFunction NULL). aFlags — exactly one of NCSDK_HOOK_REPLACES (the game's
     *  body does not run: @replaceMethod, CET Override without calling the original, a detour without the original) or
     *  NCSDK_HOOK_WRAPS (the original runs). NightCoop checks it against its own hooks like the ones it finds itself (a
     *  log line; a replacement of one co-op relies on — one note on the screen) — declare what NightCoop cannot find:
     *  natives detoured lazily, CET hooks with names built at run time, REDmod scripts. Returns 1 — co-op relies on it
     *  now, 0 — not (now: NightCoop places its hooks during the first minutes after start, see hookCoopUses);
     *  NCSDK_E_ARG, NCSDK_E_LIMIT (256 per mod), NCSDK_E_DENIED (no NCSDK_MOD_USES_HOOKS), NCSDK_E_OFF ("hook_overlap"
     *  off). Game thread. */
    int32_t (*hookDeclare)(NcSdkMod aMod, const char* aClass, const char* aFunction, uint64_t aAddress, uint32_t aFlags);
    /** Does co-op rely on this function now (the same arguments as hookDeclare)? Returns a mask of NCSDK_HOOK_AREA_*
     *  (0 — no), NCSDK_E_ARG, NCSDK_E_OFF, NCSDK_E_STATE (not the game thread). Use it to keep the original running
     *  where co-op needs it (call wrappedMethod, or skip your override while NightCoop is in a session). Game thread. */
    int32_t (*hookCoopUses)(const char* aClass, const char* aFunction, uint64_t aAddress);
    /** Keys NightCoop reads itself (the OS keyboard: a press is not consumed, the game and other mods see it too), one
     *  per line: "name\tvk\tIK_name\taction\twhen\trebindable(0/1)". Returns the full length; aOut gets it cut,
     *  zero-terminated. Needs nothing. Any thread. */
    size_t (*keysUsed)(char* aOut, size_t aCap);
    /* --- content a partner may not have (sdk/docs/content-fallbacks.md) */
    /** Name what a partner WITHOUT this mod sees instead of this mod's content aName of kind aKind (NCSDK_FALLBACK_*): the
     *  game's own content of the same kind that every player has — "Vehicle.v_my_sedan" ->
     *  "Vehicle.v_standard2_archer_hella_player", "Items.my_rifle" -> "Items.Preset_Copperhead_Default", a weather state "my_acid_rain" ->
     *  "24h_weather_toxic_rain", a radio station -> a game station, a photo mode pose animation -> a game pose animation.
     *  NightCoop sends the fallback with the content (the partner has no TweakDB record of the mod to read it from) and the
     *  partner shows it when the content is not in their game. Without a fallback NightCoop picks one itself where it can
     *  (a game vehicle of the same kind and seats, a game weapon of the same type, a weather state of the same family by
     *  name, a plain photo mode pose, the partner's own radio station). aFallback NULL or "" — clear. Names: a TweakDB
     *  record name for vehicles and items, the CName for the rest; 1..NCSDK_FALLBACK_NAME_MAX bytes of printable ASCII.
     *  Name fallbacks right after registerMod; unregistering the mod drops them. Returns NCSDK_OK, NCSDK_E_ARG (bad handle,
     *  kind or name, a fallback to itself), NCSDK_E_DENIED (no NCSDK_MOD_USES_FALLBACKS), NCSDK_E_LIMIT
     *  (NCSDK_FALLBACKS_MAX per mod), NCSDK_E_OFF (the player switched "sdk_fallbacks" off). Any thread. */
    int32_t (*fallbackSet)(NcSdkMod aMod, int32_t aKind, const char* aName, const char* aFallback);
    /** Is content aName of kind aKind (NCSDK_FALLBACK_*) in THIS game: 1 yes, 0 no. NCSDK_E_ARG — bad kind or name;
     *  NCSDK_E_STATE — cannot tell yet (no TweakDB; the game's radio stations or photo mode poses not read yet);
     *  NCSDK_E_UNSUPPORTED — weather states (the game has no list to ask; a guest learns only by setting one). Use it on
     *  names a partner's copy of your mod sends you. Game thread. */
    int32_t (*contentHere)(int32_t aKind, const char* aName);
    /** Does the partner aPeer have a mod that does not use the SDK, by where it lies (aKind — NCSDK_KNOWN_MOD_*) and its
     *  folder or file name as in NightCoop's mods list (logs/nightcoop-mods.txt; no case, "/" or "\\"): "RadioExt",
     *  "AppearanceMenuMod", "ArchiveXL.dll"… 1 yes, 0 no, NCSDK_E_STATE — not known yet (the mods check with them is not
     *  over, or switched off: "mods_check"), NCSDK_E_ARG, NCSDK_E_UNKNOWN (no such peer). Only reading, needs nothing.
     *  Game thread. */
    int32_t (*peerHasKnownMod)(NcSdkPeer aPeer, int32_t aKind, const char* aName);
} NcSdkApi;

enum
{
    /** send(): reliable and ordered per channel (default — unreliable). */
    NCSDK_SEND_RELIABLE = 1u << 0,
    /** send() unreliable: drop a not yet sent older message of the same channel (streams of states). */
    NCSDK_SEND_LATEST_ONLY = 1u << 1,
};

typedef const NcSdkApi* (*NcSdkGetFn)(uint32_t aVersion);

/* Limits of factKeepLocal */
#define NCSDK_FACT_NAME_MAX 128    /**< quest fact name, bytes (printable ASCII, no spaces) */
#define NCSDK_LOCAL_FACTS_MAX 1024 /**< per-player facts per mod */

/* The default length of cleanText (aMaxBytes 0), bytes of UTF-8 */
#define NCSDK_TEXT_MAX 256

/* Limits of scriptableScope, the co-op menu and votes */
#define NCSDK_CLASS_NAME_MAX 128  /**< scriptableScope class name, bytes (printable ASCII) */
#define NCSDK_SCOPES_MAX 32       /**< scriptableScope words per mod */
#define NCSDK_MENU_KEY_MAX 32     /**< menu item key, bytes ([a-z0-9._-]) */
#define NCSDK_MENU_LABEL_MAX 80   /**< menu item label, bytes of UTF-8 */
#define NCSDK_MENU_ITEMS_MAX 8    /**< menu items per mod */
#define NCSDK_VOTE_KEY_MAX 32     /**< vote key, bytes ([a-z0-9._-]) */
#define NCSDK_VOTE_TEXT_MAX 160   /**< the question, bytes of UTF-8 per language */
#define NCSDK_VOTE_OPTION_MAX 80  /**< an option, bytes of UTF-8 per language */
#define NCSDK_VOTE_OPTIONS_MAX 8  /**< options of a vote */
/* Shared spawns (sdk/docs/shared-spawns.md).
 * The id of an NCSDK_NET_SPAWN reference carries the sharing machine's peer number in its top byte, so one
 * reference names one spawn on every machine of the session (entitySetState works on shared spawns). It is
 * session-scoped: take entityRef again after NCSDK_EV_SESSION_START. A shared vehicle stays an NCSDK_NET_SPAWN reference
 * even while NightCoop mirrors it as a shared vehicle (seats, doors, driver). */
enum
{
    /** entityShare of an NPC: NightCoop may hand it to the player nearest to it, as it does with the NPCs of the world —
     *  that machine's AI leads it then (an ambush around a far guest is fought by the guest's game, not by the host's
     *  far from its camera). Without it the machine that shared it always leads (a companion whose role and looks your mod
     *  keeps). While a partner leads it your entity is its mirror: do not drive it, ask the leader (a channel message).
     *  hooks.spawnLeader / NCSDK_POLL_SPAWN_LEADER tell when the leader changes; entityLeader and entityIsMine tell now.
     *  Ignored for props and vehicles (a shared vehicle is always led by its driver). Switch: "sdk_spawns_handoff". */
    NCSDK_SHARE_HANDOFF = 1u << 2,
};

enum
{
    /** pollEvent: peer = the new leader, entity = our entity of the spawn, ref, a = 1 this machine leads it now / 0 a
     *  partner does (hooks.spawnLeader). */
    NCSDK_POLL_SPAWN_LEADER = 9,
};

enum
{
    /** Passport flag: the mod changes how NPCs think or fight (AI, balance, police, reactions). NightCoop runs an NPC's AI
     *  only on the machine that leads it (partners show its copy), so an NPC of a player who has such a mod is handed to
     *  the nearest player only if that player has the same mod (same id and protocol); a player without it shows its copy.
     *  Switch: "sdk_npc_logic". */
    NCSDK_MOD_CHANGES_NPC_LOGIC = 1u << 20,
    /** Permission: effectShow, effectAttach, effectStop, soundPlay; partners' effects of mods are shown only from a peer whose
     *  passport declares it. */
    NCSDK_MOD_USES_EFFECTS = 1u << 21,
    /** Permission: rewardGrant; partners' rewards are given only from a peer whose passport declares it. */
    NCSDK_MOD_USES_REWARDS = 1u << 22,
};

enum
{
    /** effectShow: the effect runs in real time, not slowed by the world's time dilation (FxSystem.SpawnEffect's
     *  ignoreTimeDilation). */
    NCSDK_FX_IGNORE_TIME_DILATION = 1u << 0,
    /** effectAttach: aEffect is the name of an effect of the entity's template, not a resource path. */
    NCSDK_FX_BY_NAME = 1u << 1,
};

enum
{
    /** pollEvent: the answer to rewardGrant — peer = the recipient, entity = the grant number, a = NCSDK_OK or NCSDK_E_*
     *  (hooks.reward). */
    NCSDK_POLL_REWARD = 18,
};

#define NCSDK_REWARD_MAX 9999 /**< rewardGrant: quantity per grant */
#define NCSDK_LEASE_MAX_S 600 /**< npcLease: the longest lease in seconds */
#define NCSDK_LEASE_UNTIL_RELEASE 0xFFFFFFFFu /**< npcLease: until the mod releases it (aSeconds 0) */
#define NCSDK_REWARD_PER_S 2  /**< rewardGrant: per mod (bursts of 8); a partner refuses a peer's rewards over it */

#define NCSDK_FX_PATH_MAX 255 /**< effectShow: resource path, bytes */
#define NCSDK_FX_PER_S 10     /**< effectShow: per mod, bursts up to 20; a partner drops a peer's mod effects over 20/s */

/* hookDeclare / hookCoopUses */
enum
{
    /** Passport flag: the mod declares its hooks (hookDeclare). */
    NCSDK_MOD_USES_HOOKS = 1u << 18,
    /** hookDeclare: the game's body does not run (a replacement). */
    NCSDK_HOOK_REPLACES = 1u << 0,
    /** hookDeclare: the original runs (a wrapper or an observer). */
    NCSDK_HOOK_WRAPS = 1u << 1,
};

/** hookCoopUses: what co-op does there (sdk/docs/do-not-hook.md has the table per area). */
enum
{
    NCSDK_HOOK_AREA_PLAYER = 1u << 0,     /**< player body and state (partners' bodies are PlayerPuppets) */
    NCSDK_HOOK_AREA_NPC = 1u << 1,        /**< NPCs, AI, combat, police */
    NCSDK_HOOK_AREA_POPULATION = 1u << 2, /**< population and crowd */
    NCSDK_HOOK_AREA_VEHICLES = 1u << 3,
    NCSDK_HOOK_AREA_QUESTS = 1u << 4,     /**< quests, facts, scenes, dialogs */
    NCSDK_HOOK_AREA_TIME = 1u << 5,       /**< time and pause */
    NCSDK_HOOK_AREA_SAVES = 1u << 6,      /**< saves and loading */
    NCSDK_HOOK_AREA_MENU = 1u << 7,       /**< menus and HUD */
    NCSDK_HOOK_AREA_WORLD = 1u << 8,      /**< world, physics, effects */
    NCSDK_HOOK_AREA_DEVICES = 1u << 9,    /**< devices, doors, lifts */
    NCSDK_HOOK_AREA_OTHER = 1u << 10,
};

/* Kinds of content for fallbackSet and contentHere */
enum
{
    NCSDK_FALLBACK_VEHICLE = 1,    /**< a vehicle record (TweakDB, "Vehicle.*") */
    NCSDK_FALLBACK_ITEM = 2,       /**< an item record (TweakDB, "Items.*"): weapons in hands, clothes, items */
    NCSDK_FALLBACK_WEATHER = 3,    /**< a weather state name (worldWeatherState.name), e.g. "24h_weather_rain" */
    NCSDK_FALLBACK_RADIO = 4,      /**< a radio station: name — what the car's GetRadioReceiverStationName gives while the
                                        mod's station plays; fallback (and contentHere) — a game station by its
                                        ERadioStationList name, e.g. "AGGRO_INDUSTRIAL" */
    NCSDK_FALLBACK_PHOTO_POSE = 5, /**< a photo mode pose by its animation (PhotoModePoses.*.animationName), e.g. "idle_stand_02" */
};
#define NCSDK_FALLBACK_NAME_MAX 128 /**< a name in fallbackSet / contentHere, bytes of printable ASCII */
#define NCSDK_FALLBACKS_MAX 1024    /**< fallbacks per mod */

/* Where a mod without the SDK lies, for peerHasKnownMod (the kinds of NightCoop's mods
 * list, logs/nightcoop-mods.txt) */
enum
{
    NCSDK_KNOWN_MOD_RED4EXT = 0,   /**< red4ext/plugins/<folder or .dll> */
    NCSDK_KNOWN_MOD_REDSCRIPT = 1, /**< r6/scripts/<folder or .reds> */
    NCSDK_KNOWN_MOD_CET = 2,       /**< bin/x64/plugins/cyber_engine_tweaks/mods/<folder> */
    NCSDK_KNOWN_MOD_ARCHIVE = 3,   /**< archive/pc/mod/<path of the .archive or .xl, "/"-separated> */
    NCSDK_KNOWN_MOD_REDMOD = 4,    /**< mods/<folder> */
    NCSDK_KNOWN_MOD_TWEAK = 5,     /**< r6/tweaks/<folder or .yaml/.yml/.tweak> */
    NCSDK_KNOWN_MOD_ASI = 6,       /**< bin/x64/plugins/<.asi> */
};

/* entityAirborne aOn bits */
enum
{
    NCSDK_AIRBORNE_HOLD = 1u << 0,      /**< the mod holds it in the air: ground rules leave it alone */
    NCSDK_AIRBORNE_NO_WHEELS = 1u << 1, /**< the mod hides its wheels while it flies: a partner without the mod sees it
                                             without wheels too */
};

#ifdef __cplusplus
}
#endif
