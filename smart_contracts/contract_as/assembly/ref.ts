/**
 * Boxes a value which could then be nullable in any context.
 */
export class Ref<T> {
    constructor(public value: T) {}
}
