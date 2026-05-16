function ArrayFixtureUse() {
    var ints : array<int>;
    var entities : array<CEntity>;
    var nested : array<array<int>>;
    var size : int;
    var head : CEntity;

    ints.PushBack(1);
    ints.Resize(10);
    ints.Clear();

    if (ints.Contains(5)) {
        ints.Remove(5);
    }

    size = ints.Size();
    head = entities.Last();
    nested.PushBack(ints);
}
