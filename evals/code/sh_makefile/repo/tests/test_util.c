#include <stdio.h>
#include "../src/util.h"

static int failures;

static void expect(long a, char op, long b, int want_err, long want)
{
	long got = 0;
	int err = apply(a, op, b, &got);

	if (err != want_err || (err == 0 && got != want)) {
		printf("FAIL %ld %c %ld: err %d, got %ld\n", a, op, b, err, got);
		failures++;
	}
}

int main(void)
{
	expect(2, '+', 3, 0, 5);
	expect(2, '-', 3, 0, -1);
	expect(7, '*', 6, 0, 42);
	expect(9, '/', 2, 0, 4);
	expect(1, '/', 0, -2, 0);
	expect(1, '%', 1, -1, 0);
	if (failures) {
		printf("%d failures\n", failures);
		return 1;
	}
	printf("ok\n");
	return 0;
}
