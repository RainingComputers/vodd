static int twice(int value)
{
    int held = value + value;

    return held;
}

static int shift(int value, int by)
{
    int held = twice(value);

    return held + by;
}

kernel void nested(global const int *in, global int *out)
{
    int gid = get_global_id(0);
    int seen = in[gid];

    out[gid] = shift(seen, gid);
}
