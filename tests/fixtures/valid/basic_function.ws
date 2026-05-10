function BasicLightRewriteExample(owner : CObject) : bool {
    var count : int;
    var enabled : bool;

    count = 1;
    enabled = count > 0;

    if (enabled) {
        return true;
    }

    return false;
}
