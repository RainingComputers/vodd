kernel void mix(global const float *in, global float *out, local float *tile)
{
    int lid = get_local_id(0);
    int gid = get_global_id(0);

    float scale = 2.5f;
    float seen = in[gid] + scale;
    float4 packed = (float4)(seen, seen + 1.0f, 2.0f, 3.0f);

    tile[lid] = seen;
    barrier(CLK_LOCAL_MEM_FENCE);

    out[gid] = tile[lid] + packed.x;
}
