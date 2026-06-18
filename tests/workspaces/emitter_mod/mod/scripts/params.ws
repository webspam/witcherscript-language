// Per-emitter tuning parameters. Every optional field is paired with a has*
// guard; a false guard means "leave whatever the engine set in place".

enum EEmitterKind {
	EK_None,
	EK_Bell,
	EK_Chime,
	EK_Drum
}

abstract class IEmitterParams {
	public var hasVolume : bool;
	public var volume : float;

	public var hasRadius : bool;
	public var radius : float;

	public var hasPitch : bool;
	public var pitch : float;
}

class CEmitterParams extends IEmitterParams {
	public var tag : name;
	public var displayName : string;
	default displayName = "generic";

	public var weight : int;
	public var profileName : name;

	public var hasKind : bool;
	public var kind : EEmitterKind;

	// True when the host carries a name the override is allowed to touch.
	public function MatchesEntity(host : CWorldEntity) : bool {
		return host.GetDisplayName() != "";
	}

	// Copies every set field onto target, overwriting whatever it held.
	public function ApplyTo(target : CEmitterParams) {
		if (hasVolume) {
			target.hasVolume = true;
			target.volume = volume;
		}
		if (hasRadius) {
			target.hasRadius = true;
			target.radius = radius;
		}
		if (hasPitch) {
			target.hasPitch = true;
			target.pitch = pitch;
		}
		if (hasKind) {
			target.hasKind = true;
			target.kind = kind;
		}
	}
}
