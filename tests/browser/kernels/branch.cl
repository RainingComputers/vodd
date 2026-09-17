kernel void branch(global const int *in, global int *out)
{
    int lid = get_local_id(0);
    int gid = get_global_id(0);
    int seen = in[gid];

    if (lid < 16) {
        seen = seen + 1;
    } else {
        seen = seen - 1;
    }

    out[gid] = seen;
}
