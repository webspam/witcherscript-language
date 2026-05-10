enum EParserFixtureKind {
    PFK_None,
    PFK_Candle = 1,
    PFK_Torch = 2,
}

struct SParserFixtureOriginalValues {
    var hasRadius : bool;
    var radius : float;
}

abstract class IParserFixtureParams {
    public var enabled : bool;
    public var radius : float;
}

class CParserFixtureParams extends IParserFixtureParams {
    public const var TAG_ENABLED : name; default TAG_ENABLED = 'ParserFixtureEnabled';
    public const var TAG_RADIUS : name;  default TAG_RADIUS = 'ParserFixtureRadius';

    defaults {
        enabled = true;
        radius = 6.0;
    }
}

@adField(CGameplayEntity) public var parserFixtureParams : CParserFixtureParams;

@wrapMethod(CR4Player)
protected function ParserFixtureWrapped(spawnData : SEntitySpawnData) {
    var params : CParserFixtureParams;

    wrappedMethod(spawnData);
    params = new CParserFixtureParams in this;
    this.parserFixtureParams = params;
}

@addMethod(CR4Player)
timer function ParserFixtureTimer(dt : float, id : int) {
    var i, count : int;
    var entities : array<CGameplayEntity>;

    count = entities.Size();
    for (i = 0; i < count; i += 1) {
        if (entities[i]) {
            break;
        }
    }
}
