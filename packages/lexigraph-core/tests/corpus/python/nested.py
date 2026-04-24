def deep():
    if True:
        for i in range(3):
            while i > 0:
                try:
                    if i % 2 == 0:
                        return [[[[[[i]]]]]]
                    else:
                        i -= 1
                except Exception:
                    pass
                finally:
                    pass
    return None
