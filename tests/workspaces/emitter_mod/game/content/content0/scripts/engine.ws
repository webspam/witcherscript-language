// De-identified stand-ins for the engine/base-script layer the mod builds on.
// They mirror the shape of real base scripts: a node/entity hierarchy plus the
// components the mod reaches through, so the mod resolves without the real game.

abstract class CWorldNode {
	var nodeName : string;

	function GetDisplayName() : string {
		return nodeName;
	}
}

class CWorldComponent {
	function SetEnabled(enable : bool) {}

	function IsEnabled() : bool {
		return true;
	}
}

class CAudioComponent extends CWorldComponent {
	var volume : float;
	var radius : float;
	var pitch : float;

	function ResetToDefaults() {}
}

class CWorldEntity extends CWorldNode {
	function AddTag(tag : name) {}

	function GetComponentByName(componentName : name) : CWorldComponent {
		return NULL;
	}

	function GetComponentsByName(componentName : name) : array<CWorldComponent> {
		var found : array<CWorldComponent>;
		return found;
	}
}

class CWorld {
	function CollectEntitiesByTag(tag : name, out result : array<CWorldEntity>) {}

	function CollectNodesByTags(tags : array<name>, out result : array<CWorldNode>) {}
}
