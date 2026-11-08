int depth(int n);

__attribute__((noinline)) int trap_here(int n) {
    if (n > 2) {
        __builtin_trap();
    }
    return n + 1;
}

__attribute__((noinline)) int depth(int n) {
    return trap_here(n) * 2;
}

int main(void) {
    return depth(3);
}
