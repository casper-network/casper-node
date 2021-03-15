

def is_valid_key(key_hex: str) -> bool:
    key_length = {"01": 66,
                  "02": 68}
    start = key_hex[:2]
    try:
        length = key_length[start]
        if len(key_hex) != length:
            return False
        _ = int(key_hex, 16)
    except KeyError:
        # Key doesn't start with "01" or "02"
        return False
    except ValueError:
        # Key isn't hex as int() failed
        return False
    return True
