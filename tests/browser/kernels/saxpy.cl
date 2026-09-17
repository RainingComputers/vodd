kernel void saxpy(global const float *x, global float *y)
{
    int i = get_global_id(0);

    y[i] = 2.0f * x[i] + y[i];
}
