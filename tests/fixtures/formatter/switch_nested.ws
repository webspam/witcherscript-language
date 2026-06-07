function F() {
    switch (x) {
        case 0:  Foo();  break;
        case 1:
            switch (y) {
                case 2:  G();  break;
            }
            break;
    }
}
