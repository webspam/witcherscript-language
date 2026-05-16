// Built-in: the bare ident `T` inside this class represents the array element type.
// The LSP substitutes `T` with the actual element type when resolving members of `array<X>`.
import class array {
    import function Clear() : void;
    import function Contains(param1 : T) : bool;
    import function Erase(param1 : int) : void;
    import function EraseFast(param1 : int) : void;
    import function FindFirst(param1 : T) : int;
    import function Grow(param1 : int) : void;
    import function Insert(param1 : int, param2 : T) : void;
    import function Last() : T;
    import function PopBack() : void;
    import function PushBack(param1 : T) : void;
    import function Remove(param1 : T) : bool;
    import function Resize(param1 : int) : void;
    import function Size() : int;
}
