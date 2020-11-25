import { ErrorCode, Error as StdError } from "./error";
import { Ref } from "./ref";

/**
 * Class representing a result of an operation that might have failed. Can contain either a value
 * resulting from a successful completion of a calculation, or an error. Similar to `Result` in Rust
 * or `Either` in Haskell.
 */
export class Result<T> {
    /**
     * Creates new Result with wrapped value
     * @param value Ref-wrapped value (success) or null (error)
     * @param error Error value
     */
    constructor(public ref: Ref<T> | null, public error: StdError) {}

    /**
     * Assumes that reference wrapper contains a value and then returns it
     */
    get value(): T {
        assert(this.hasValue());
        let ref = <Ref<T>>this.ref;
        return ref.value;
    }

    /**
     * Checks if given Result contains a value
     */
    hasValue(): bool {
        return this.ref !== null;
    }

    /**
     * Checks if error value is set.
     *
     * Truth also implies !hasValue(), false value implies hasValue()
     */
    hasError(): bool {
        return this.error != Error.Ok;
    }

    /**
     * For nullable types, this returns the value itself, or a null.
     */
    ok(): T | null {
        return this.hasValue() ? this.value : null;
    }

    /**
     * Returns success value, or reverts error value.
     */
    unwrap(): T {
        if (this.error !== null) {
            this.error.revert();
            return <T>unreachable();
        }
        return this.value;
    }
}