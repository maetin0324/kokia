#include <stdio.h>

void foo() {
    printf("In foo()\n");
}

void bar() {
    printf("In bar()\n");
}

int main() {
    printf("Starting...\n");
    foo();
    bar();
    printf("Done!\n");
    return 0;
}
