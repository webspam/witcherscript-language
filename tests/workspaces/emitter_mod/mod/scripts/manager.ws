// Owns tuner lifecycle: builds the right tuner for a params kind and re-applies
// tuning across every tagged host in the world.

class CEmitterManager {
	public const var HOST_TAG : name;
	default HOST_TAG = 'AA_HasEmitter';

	public var world : CWorld;
	public var gameStarted : bool;

	public function Init(world : CWorld) {
		this.world = world;
	}

	// Picks a concrete tuner for the params kind and initialises it on the host.
	public function CreateTunerFromParams(params : CEmitterParams, host : CWorldEntity) : IEmitterTuner {
		var tuner : IEmitterTuner;

		switch (params.kind) {
			case EK_Bell:
				tuner = new CBellTuner in host;
				break;
			default:
				tuner = new CGenericTuner in host;
				break;
		}

		tuner.Init(host, params, NULL);
		return tuner;
	}

	// Rebuilds and applies a tuner for every tagged host.
	public function RetuneAll(params : CEmitterParams) {
		var hosts : array<CWorldEntity>;
		var tuner : IEmitterTuner;
		var i, count : int;

		CollectHosts(hosts);
		count = hosts.Size();

		for (i = 0; i < count; i += 1) {
			tuner = CreateTunerFromParams(params, hosts[i]);
			tuner.TuneEmitter();
		}
	}

	private function CollectHosts(out hosts : array<CWorldEntity>) {
		var nodes : array<CWorldNode>;
		var entity : CWorldEntity;
		var i, count : int;
		var tags : array<name>;

		tags.PushBack(HOST_TAG);
		world.CollectNodesByTags(tags, nodes);

		count = nodes.Size();
		for (i = 0; i < count; i += 1) {
			entity = (CWorldEntity)nodes[i];
			if (entity) {
				hosts.PushBack(entity);
			}
		}
	}
}
