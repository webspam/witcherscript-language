// Built-in: the bare ident `T` inside this class represents the array element type.
// The LSP substitutes `T` with the actual element type when resolving members of `array<X>`.
import class array {
    import function Clear() : void;
    import function Contains(value : T) : bool;
    import function Erase(index : int) : void;
    import function EraseFast(param1 : int) : void;
    import function FindFirst(value : T) : int;
    import function Grow(newSize : int) : void;
    import function Insert(index : int, value : T) : void;
    import function Last() : T;
    import function PopBack() : void;
    import function PushBack(value : T) : void;
    import function Remove(value : T) : bool;
    import function Resize(newSize : int) : void;
    import function Size() : int;
}
