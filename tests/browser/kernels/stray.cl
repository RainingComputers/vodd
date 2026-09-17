kernel void stray(global const int *in, global int *out)
{
    int i = get_global_id(0);

    if (i == 130) {
        out[i + 1000000] = i;
    }
}
