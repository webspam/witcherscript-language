// Positional-probe host: the cursor sits inside the argument list of a two-arg
// call, on the second argument, so signature help reports an active parameter.
function ProbeTunerSignature(manager : CEmitterManager, params : CEmitterParams, host : CWorldEntity) {
	manager.CreateTunerFromParams(params, $0host);
}
