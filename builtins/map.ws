import class map {
    import function Clear() : void;
    import function Contains(key : K) : bool;
    import function Erase(key : K) : bool;
    import function Key(index : int) : K;
    import function Size() : int;
    import function TryGet(key : K, out value : V) : bool;
}
