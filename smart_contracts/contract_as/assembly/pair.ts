/**
 * A pair of values.
 *
 * @typeParam T1 The type of the first value.
 * @typeParam T2 The type of the second value.
 */
export class Pair<T1, T2> {
	/**
	 * The first value in the pair.
	 */
    public first: T1;
	/**
     * The second value in the pair.
	 */
    public second: T2;

	/**
	 * Constructs the pair out of the two given values.
	 */
    constructor(first: T1, second: T2) {
        this.first = first;
        this.second = second;
    }

	/**
	 * Checks whether two pairs are equal. The pairs are considered equal when both their first
	 * and second values are equal.
	 */
    @operator("==")
    equalsTo(other: Pair<T1, T2>): bool {
        return this.first == other.first && this.second == other.second;
    }

	/**
	 * Checks whether two pairs are not equal (the opposite of [[equalsTo]]).
	 */
    @operator("!=")
    notEqualsTo(other: Pair<T1, T2>): bool {
        return !this.equalsTo(other);
    }
}
