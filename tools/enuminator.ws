// Enuminator.ws - logs all enum members to scriptslog.txt after loading the menu
// Game must be started with args: `-net -debugscripts`

// Replace all (2) instances of `EInputKey` with the enum to enumerate.
function ToEnumMember(i: int): string {
    return "" + (EInputKey)i;
}

@wrapMethod(CR4IngameMenu)
function OnConfigUI() {
    var enumName: name = 'EInputKey';

    var i: int = EnumGetMin(enumName);
    var max: int = EnumGetMax(enumName);

    LogChannel('EnuminatorMin', "EnumGetMin(" + enumName + "): " + i);
    LogChannel('EnuminatorMax', "EnumGetMax(" + enumName + "): " + max);

    // Do not refactor to modulo; can't handle large ints: e.g. (0x40000000 % 2) returns `21`
    // Integer overflow protection - see `EDialogActionIcon`
    if (Abs(max - i) > 16384 || max - i > 16384) {
        // Veeeery likely to be bit flags
        EnuminateBitFlags(i, max);
    }
    else {
        // Explicitly enumerate enum
        EnuminateEnum(i, max);
    }

    wrappedMethod();
}

function EnuminateBitFlags(i: int, max: int) {
    var enumMember: string;

    while (i <= max) {
        enumMember = ToEnumMember(i);
        if (enumMember != "") LogChannel('Enuminator', enumMember + " = " + i);

        // Integer overflow protection - see `EDialogActionIcon`
        if (i < -1073741824) i = -1073741824;
        else if (i >= 1073741824) break;
        else if (i < 0) i /= 2;
        else if (i == 0) i = 1;
        else i *= 2;
    }
}

function EnuminateEnum(i: int, max: int) {
    var enumMember: string;

    for (; i <= max; i += 1) {
        enumMember = ToEnumMember(i);
        if (enumMember != "") LogChannel('Enuminator', enumMember + " = " + i);
    }
}
