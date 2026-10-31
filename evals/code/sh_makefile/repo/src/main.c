#include <stdio.h>
#include <stdlib.h>
#include "util.h"

int main(int argc, char **argv)
{
	long r;
	int err;

	if (argc != 4 || argv[2][0] == '\0' || argv[2][1] != '\0') {
		fprintf(stderr, "usage: calc A OP B\n");
		return 2;
	}
	err = apply(strtol(argv[1], NULL, 10), argv[2][0], strtol(argv[3], NULL, 10), &r);
	if (err == -1) {
		fprintf(stderr, "calc: unknown operator %s\n", argv[2]);
		return 1;
	}
	if (err == -2) {
		fprintf(stderr, "calc: division by zero\n");
		return 1;
	}
	printf("%ld\n", r);
	return 0;
}
