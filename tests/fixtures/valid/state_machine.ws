statemachine class ParserFixtureOneliner extends SU_Oneliner {
    public var entity : CGameplayEntity;

    public function Show(entity : CGameplayEntity) {
        this.entity = entity;
        this.GotoState('FollowEntity');
    }
}

state Idle in ParserFixtureOneliner {}

state FollowEntity in ParserFixtureOneliner {
    event OnEnterState(previous_state_name : name) {
        super.OnEnterState(previous_state_name);
        parent.Start();
    }

    event OnLeaveState(next_state_name : name) {
        parent.Stop();
        super.OnLeaveState(next_state_name);
    }

    entry function FollowEntity() : void {
        var startTime, now : float;

        startTime = EngineTimeToFloat(theGame.GetEngineTime());
        now = startTime;

        while ((now - startTime) < 1.0 && parent.entity) {
            SleepOneFrame();
            now = EngineTimeToFloat(theGame.GetEngineTime());
        }

        parent.GotoState('Idle');
    }
}
