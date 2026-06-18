// Positional-probe host. The cursor sits on a cross-file method call, so one
// position drives definition, type-definition, hover, references and highlights.
function ProbeTunerLookup(manager : CEmitterManager, params : CEmitterParams, host : CWorldEntity) : IEmitterTuner {
	return manager.$0CreateTunerFromParams(params, host);
}
