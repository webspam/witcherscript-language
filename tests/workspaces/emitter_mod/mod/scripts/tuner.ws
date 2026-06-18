// Abstract base for emitter tuners. Subclasses implement TuneEmitter(); the base
// owns the params, the effective-override resolution, and shared component work.

abstract class IEmitterTuner {
	public var kind : EEmitterKind;
	public var host : CWorldEntity;

	protected var params : CEmitterParams;
	protected var overrideParams : CEmitterParams;

	// Lazy constructor; subclasses that override must call super.Init().
	public function Init(host : CWorldEntity, params : CEmitterParams, globalOverride : CEmitterParams) {
		this.host = host;
		this.params = params;

		host.AddTag(params.tag);
		if (globalOverride) {
			SetGlobalOverride(globalOverride);
		}
	}

	public function SetGlobalOverride(params : CEmitterParams) {
		if (params.hasVolume) {
			overrideParams = params;
		} else {
			overrideParams = NULL;
		}
	}

	protected function GetEffectiveParams() : CEmitterParams {
		if (overrideParams) {
			return overrideParams;
		}
		return params;
	}

	// Abstract: each kind applies its own component changes.
	public function TuneEmitter();

	// Virtual hook run once the world is live; does nothing by default.
	public function ProcessQueued() {}

	// Shared application of params onto any audio component.
	protected function ApplyParams(component : CAudioComponent, source : IEmitterParams) {
		if (source.hasVolume) {
			component.volume = source.volume;
		}
		if (source.hasRadius) {
			component.radius = source.radius;
		}
		if (source.hasPitch) {
			component.pitch = source.pitch;
		}
	}

	// Restores every audio component on the host to its engine defaults.
	public function RestoreOriginal() {
		var component : CAudioComponent;
		var i, count : int;

		var components : array<CWorldComponent> = host.GetComponentsByName('CAudioComponent');
		count = components.Size();

		for (i = 0; i < count; i += 1) {
			component = (CAudioComponent)components[i];
			if (component) {
				component.ResetToDefaults();
			}
		}
	}
}

class CBellTuner extends IEmitterTuner {
	private var slotNames : array<name>;

	public function Init(host : CWorldEntity, params : CEmitterParams, globalOverride : CEmitterParams) {
		super.Init(host, params, globalOverride);
		slotNames.PushBack('bell');
	}

	public function TuneEmitter() {
		var p : CEmitterParams = GetEffectiveParams();
		var component : CAudioComponent;

		if (slotNames.Size() == 0) {
			return;
		}

		component = (CAudioComponent)host.GetComponentByName('CAudioComponent');
		if (!component) {
			return;
		}

		ApplyParams(component, p);
		if (p.hasPitch) {
			component.pitch = p.pitch * 2;
		}
	}
}

class CGenericTuner extends IEmitterTuner {
	public function TuneEmitter() {
		var component : CAudioComponent = (CAudioComponent)host.GetComponentByName('CAudioComponent');
		if (component) {
			ApplyParams(component, GetEffectiveParams());
		}
	}
}
