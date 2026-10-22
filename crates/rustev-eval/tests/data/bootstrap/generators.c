/*
 * Known-answer vectors for spec 029, 3.4.2: `generators.json`.
 *
 * The two `next` functions below are the authors' reference
 * implementations, unchanged apart from their names and the state passed
 * by pointer: splitmix64.c and xoshiro256starstar.c by David Blackman and
 * Sebastiano Vigna (https://prng.di.unimi.it/), dedicated to the public
 * domain (CC0). Only the driver that prints the vectors is new.
 *
 * Regenerate: cc -std=c99 -O0 -o generators generators.c && ./generators
 * > generators.json
 */
#include <stdint.h>
#include <stdio.h>

/* splitmix64.c */
static uint64_t splitmix64_next(uint64_t *x) {
	uint64_t z = (*x += 0x9e3779b97f4a7c15);
	z = (z ^ (z >> 30)) * 0xbf58476d1ce4e5b9;
	z = (z ^ (z >> 27)) * 0x94d049bb133111eb;
	return z ^ (z >> 31);
}

/* xoshiro256starstar.c */
static inline uint64_t rotl(const uint64_t x, int k) {
	return (x << k) | (x >> (64 - k));
}

static uint64_t xoshiro_next(uint64_t s[4]) {
	const uint64_t result = rotl(s[1] * 5, 7) * 9;
	const uint64_t t = s[1] << 17;
	s[2] ^= s[0];
	s[3] ^= s[1];
	s[1] ^= s[2];
	s[0] ^= s[3];
	s[2] ^= t;
	s[3] = rotl(s[3], 45);
	return result;
}

#define OUTPUTS 8

int main(void) {
	const uint64_t seeds[] = {0, 1, 1234567, 0x0123456789abcdefULL, UINT64_MAX};
	const int n_seeds = sizeof seeds / sizeof seeds[0];
	const uint64_t states[][4] = {
		{1, 2, 3, 4},
		{0x9e3779b97f4a7c15ULL, 0xbf58476d1ce4e5b9ULL, 0x94d049bb133111ebULL, 0x0123456789abcdefULL},
	};
	const int n_states = sizeof states / sizeof states[0];

	printf("{\n  \"schema\": \"rustev.bootstrap-generators/1\",\n");
	printf("  \"splitmix64\": [\n");
	for (int i = 0; i < n_seeds; i++) {
		uint64_t x = seeds[i];
		printf("    { \"seed\": \"%llu\", \"outputs\": [", (unsigned long long)seeds[i]);
		for (int j = 0; j < OUTPUTS; j++)
			printf("%s\"%llu\"", j ? ", " : "", (unsigned long long)splitmix64_next(&x));
		printf("] }%s\n", i + 1 < n_seeds ? "," : "");
	}
	printf("  ],\n  \"xoshiro256starstar\": [\n");
	for (int i = 0; i < n_states; i++) {
		uint64_t s[4] = {states[i][0], states[i][1], states[i][2], states[i][3]};
		printf("    { \"state\": [\"%llu\", \"%llu\", \"%llu\", \"%llu\"], \"outputs\": [",
		       (unsigned long long)s[0], (unsigned long long)s[1],
		       (unsigned long long)s[2], (unsigned long long)s[3]);
		for (int j = 0; j < OUTPUTS; j++)
			printf("%s\"%llu\"", j ? ", " : "", (unsigned long long)xoshiro_next(s));
		printf("] }%s\n", i + 1 < n_states ? "," : "");
	}
	/* xoshiro256** seeded with four consecutive SplitMix64 outputs. */
	printf("  ],\n  \"seeded\": [\n");
	for (int i = 0; i < n_seeds; i++) {
		uint64_t x = seeds[i], s[4];
		for (int k = 0; k < 4; k++)
			s[k] = splitmix64_next(&x);
		printf("    { \"splitmix64_seed\": \"%llu\", \"outputs\": [", (unsigned long long)seeds[i]);
		for (int j = 0; j < OUTPUTS; j++)
			printf("%s\"%llu\"", j ? ", " : "", (unsigned long long)xoshiro_next(s));
		printf("] }%s\n", i + 1 < n_seeds ? "," : "");
	}
	printf("  ]\n}\n");
	return 0;
}
