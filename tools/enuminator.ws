// Enuminator.ws - logs all enum members to scriptslog.txt after loading the menu
// Game must be started with args: `-net -debugscripts`

// Replace all (2) instances of `EInputKey` with the enum to enumerate.
function ToEnumMember(i : int) : string {
    return "" + (EInputKey) i;
}

@wrapMethod(CR4IngameMenu)
function OnConfigUI() {
    var enumName : name = 'EInputKey';

    var i : int = EnumGetMin(enumName);
    var max : int = EnumGetMax(enumName);

    LogChannel('EnuminatorMin', "EnumGetMin(" + enumName + "): " + i);
    LogChannel('EnuminatorMax', "EnumGetMax(" + enumName + "): " + max);

    if (max > 512 && max % 2 == 0) {
        // Veeeery likely to be bit flags
        EnuminateBitFlags(i, max, enumName);
    }
    else if (max - i < 16384) {
        // Explicitly enumerate enum
        EnuminateEnum(i, max, enumName);
    }
    else {
        // No thrash for you
        LogChannel('Enuminator', "Abort: > 16K entries.");
    }

    wrappedMethod();
}

function EnuminateBitFlags(i : int, max : int, enumName : name) {
    var enumMember : string;

    while (i <= max) {
        enumMember = ToEnumMember(i);
        if (enumMember != "") LogChannel('Enuminator', enumMember + " = " + i);

        if (i > 0) i *= 2;
        else if (i == 0) i = 1;
        else i /= 2;
    }
}

function EnuminateEnum(i : int, max : int, enumName : name) {
    var enumMember : string;

    for (; i <= max; i += 1) {
        enumMember = ToEnumMember(i);
        if (enumMember != "") LogChannel('Enuminator', enumMember + " = " + i);
    }
}
