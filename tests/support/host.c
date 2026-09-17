#define CL_TARGET_OPENCL_VERSION 120
#define CL_USE_DEPRECATED_OPENCL_1_2_APIS

#include <CL/cl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define MAX_ARGUMENTS 20
#define MAX_SPEC 65536

enum kind
{
    KIND_BUFFER,
    KIND_LOCAL,
    KIND_VALUE,
    KIND_NULL,
};

struct argument
{
    enum kind kind;
    size_t size;
    cl_mem_flags flags;
    cl_map_flags mapped;
    int leave_mapped;
    unsigned char *bytes;
};

static void fail(const char *what, cl_int status)
{
    fprintf(stderr, "host: %s failed with %d\n", what, status);
    exit(1);
}

static char *read_source(const char *path)
{
    FILE *file = fopen(path, "rb");
    if (!file)
    {
        fprintf(stderr, "host: cannot open %s\n", path);
        exit(1);
    }

    fseek(file, 0, SEEK_END);
    long size = ftell(file);
    fseek(file, 0, SEEK_SET);

    char *text = malloc((size_t)size + 1);
    size_t read = fread(text, 1, (size_t)size, file);
    text[read] = '\0';
    fclose(file);

    return text;
}

static void read_sizes(const char *text, size_t *out)
{
    out[0] = 1;
    out[1] = 1;
    out[2] = 1;

    char copy[64];
    strncpy(copy, text, sizeof(copy) - 1);
    copy[sizeof(copy) - 1] = '\0';

    char *token = strtok(copy, ",");
    for (int index = 0; index < 3 && token; index++)
    {
        out[index] = (size_t)strtoul(token, NULL, 10);
        token = strtok(NULL, ",");
    }
}

static unsigned char *hex_bytes(const char *text, size_t *size)
{
    size_t length = strlen(text) / 2;
    unsigned char *bytes = calloc(1, length ? length : 1);

    for (size_t index = 0; index < length; index++)
    {
        char pair[3] = {text[index * 2], text[index * 2 + 1], '\0'};
        bytes[index] = (unsigned char)strtoul(pair, NULL, 16);
    }

    *size = length;

    return bytes;
}

static void parse_argument(char *spec, struct argument *argument)
{
    memset(argument, 0, sizeof(*argument));
    argument->flags = CL_MEM_READ_WRITE;

    char *kind = strtok(spec, ":");

    if (strcmp(kind, "null") == 0)
    {
        argument->kind = KIND_NULL;
        return;
    }

    if (strcmp(kind, "local") == 0)
    {
        argument->kind = KIND_LOCAL;
        argument->size = (size_t)strtoul(strtok(NULL, ":"), NULL, 10);
        return;
    }

    if (strcmp(kind, "value") == 0)
    {
        argument->kind = KIND_VALUE;
        argument->bytes = hex_bytes(strtok(NULL, ":"), &argument->size);
        return;
    }

    argument->kind = KIND_BUFFER;
    argument->size = (size_t)strtoul(strtok(NULL, ":"), NULL, 10);
    argument->bytes = calloc(1, argument->size ? argument->size : 1);

    for (char *option = strtok(NULL, ":"); option; option = strtok(NULL, ":"))
    {
        if (strcmp(option, "ro") == 0)
        {
            argument->flags = CL_MEM_READ_ONLY;
        }
        else if (strcmp(option, "wo") == 0)
        {
            argument->flags = CL_MEM_WRITE_ONLY;
        }
        else if (strcmp(option, "map=read") == 0)
        {
            argument->leave_mapped = 1;
            argument->mapped = CL_MAP_READ;
        }
        else if (strcmp(option, "map=write") == 0)
        {
            argument->leave_mapped = 1;
            argument->mapped = CL_MAP_WRITE;
        }
        else if (strncmp(option, "data=", 5) == 0)
        {
            size_t length = 0;
            unsigned char *bytes = hex_bytes(option + 5, &length);

            memcpy(argument->bytes, bytes, length < argument->size ? length : argument->size);
            free(bytes);
        }
    }
}

static const char *USAGE =
    "usage: host [--repeat N] [--expect-local-memory BYTES]"
    " <source> <entry> <global> <local> [argument ...]\n";

int main(int argc, char **argv)
{
    int launches = 1;
    long expected_local_memory = -1;
    int first = 1;

    while (first + 1 < argc && strncmp(argv[first], "--", 2) == 0)
    {
        if (strcmp(argv[first], "--repeat") == 0)
        {
            launches = atoi(argv[first + 1]);
            if (launches < 1) launches = 1;
        }
        else if (strcmp(argv[first], "--expect-local-memory") == 0)
        {
            expected_local_memory = strtol(argv[first + 1], NULL, 10);
        }
        else
        {
            fputs(USAGE, stderr);
            return 1;
        }

        first += 2;
    }

    if (argc - first < 4)
    {
        fputs(USAGE, stderr);
        return 1;
    }

    const char *path = argv[first];
    const char *entry = argv[first + 1];
    size_t global[3];
    size_t local[3];

    read_sizes(argv[first + 2], global);
    read_sizes(argv[first + 3], local);

    int count = argc - first - 4;
    if (count > MAX_ARGUMENTS)
    {
        fprintf(stderr, "host: too many arguments\n");
        return 1;
    }

    struct argument arguments[MAX_ARGUMENTS];
    for (int index = 0; index < count; index++)
    {
        char spec[MAX_SPEC];
        strncpy(spec, argv[first + 4 + index], sizeof(spec) - 1);
        spec[sizeof(spec) - 1] = '\0';

        parse_argument(spec, &arguments[index]);
    }

    cl_int status;
    cl_platform_id platform;
    cl_device_id device;

    status = clGetPlatformIDs(1, &platform, NULL);
    if (status != CL_SUCCESS) fail("clGetPlatformIDs", status);

    status = clGetDeviceIDs(platform, CL_DEVICE_TYPE_ALL, 1, &device, NULL);
    if (status != CL_SUCCESS) fail("clGetDeviceIDs", status);

    cl_context context = clCreateContext(NULL, 1, &device, NULL, NULL, &status);
    if (status != CL_SUCCESS) fail("clCreateContext", status);

    cl_command_queue queue = clCreateCommandQueue(context, device, 0, &status);
    if (status != CL_SUCCESS) fail("clCreateCommandQueue", status);

    char *text = read_source(path);
    const char *sources[1] = {text};

    cl_program program = clCreateProgramWithSource(context, 1, sources, NULL, &status);
    if (status != CL_SUCCESS) fail("clCreateProgramWithSource", status);

    status = clBuildProgram(program, 1, &device, "-cl-opt-disable", NULL, NULL);
    if (status != CL_SUCCESS)
    {
        char log[8192] = {0};
        clGetProgramBuildInfo(program, device, CL_PROGRAM_BUILD_LOG, sizeof(log) - 1, log, NULL);
        fprintf(stderr, "host: build failed: %s\n", log);
        return 1;
    }

    cl_kernel kernel = clCreateKernel(program, entry, &status);
    if (status != CL_SUCCESS) fail("clCreateKernel", status);

    if (expected_local_memory >= 0)
    {
        cl_ulong reported = 0;

        status = clGetKernelWorkGroupInfo(kernel, device, CL_KERNEL_LOCAL_MEM_SIZE,
                                          sizeof(reported), &reported, NULL);
        if (status != CL_SUCCESS) fail("clGetKernelWorkGroupInfo", status);

        if (reported != (cl_ulong)expected_local_memory)
        {
            fprintf(stderr, "host: CL_KERNEL_LOCAL_MEM_SIZE is %llu, expected %ld\n",
                    (unsigned long long)reported, expected_local_memory);
            return 1;
        }
    }

    cl_mem buffers[MAX_ARGUMENTS] = {0};
    void *mapped[MAX_ARGUMENTS] = {0};

    for (int index = 0; index < count; index++)
    {
        struct argument *argument = &arguments[index];

        if (argument->kind == KIND_NULL)
        {
            cl_mem nothing = NULL;
            status = clSetKernelArg(kernel, (cl_uint)index, sizeof(cl_mem), &nothing);
            if (status != CL_SUCCESS) fail("clSetKernelArg", status);
            continue;
        }

        if (argument->kind == KIND_LOCAL)
        {
            status = clSetKernelArg(kernel, (cl_uint)index, argument->size, NULL);
            if (status != CL_SUCCESS) fail("clSetKernelArg", status);
            continue;
        }

        if (argument->kind == KIND_VALUE)
        {
            status = clSetKernelArg(kernel, (cl_uint)index, argument->size, argument->bytes);
            if (status != CL_SUCCESS) fail("clSetKernelArg", status);
            continue;
        }

        buffers[index] = clCreateBuffer(context, argument->flags, argument->size, NULL, &status);
        if (status != CL_SUCCESS) fail("clCreateBuffer", status);

        status = clEnqueueWriteBuffer(queue, buffers[index], CL_TRUE, 0, argument->size,
                                      argument->bytes, 0, NULL, NULL);
        if (status != CL_SUCCESS) fail("clEnqueueWriteBuffer", status);

        if (argument->leave_mapped)
        {
            mapped[index] = clEnqueueMapBuffer(queue, buffers[index], CL_TRUE, argument->mapped, 0,
                                               argument->size, 0, NULL, NULL, &status);
            if (status != CL_SUCCESS) fail("clEnqueueMapBuffer", status);
        }

        status = clSetKernelArg(kernel, (cl_uint)index, sizeof(cl_mem), &buffers[index]);
        if (status != CL_SUCCESS) fail("clSetKernelArg", status);
    }

    int dimensions = (global[2] > 1) ? 3 : ((global[1] > 1) ? 2 : 1);

    for (int launch = 0; launch < launches; launch++)
    {
        status = clEnqueueNDRangeKernel(queue, kernel, (cl_uint)dimensions, NULL, global, local, 0,
                                        NULL, NULL);
        if (status != CL_SUCCESS) fail("clEnqueueNDRangeKernel", status);
    }

    status = clFinish(queue);
    if (status != CL_SUCCESS) fail("clFinish", status);

    for (int index = 0; index < count; index++)
    {
        if (mapped[index])
        {
            clEnqueueUnmapMemObject(queue, buffers[index], mapped[index], 0, NULL, NULL);
        }

        if (buffers[index])
        {
            clReleaseMemObject(buffers[index]);
        }

        free(arguments[index].bytes);
    }

    clFinish(queue);
    clReleaseKernel(kernel);
    clReleaseProgram(program);
    clReleaseCommandQueue(queue);
    clReleaseContext(context);
    free(text);

    return 0;
}
