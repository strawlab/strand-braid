from setuptools import setup, find_packages
import os

backend = 'backend_' + os.environ['BACKEND']
ipp_sys = os.environ['IPP_SYS']

def build_native(spec):
    # Step 1: build the rust library
    build = spec.add_external_build(
        cmd=['cargo', 'build', '--release', '--features',
            '%s ipp-sys/%s'%(backend,ipp_sys)],
        path='./rust'
    )

    # Step 2: add a cffi module based on the dylib we built
    #
    # We use lambdas here for dylib and header_filename so that those are
    # only called after the external build finished.
    spec.add_cffi_module(
        module_path='strandcam._native',
        dylib=lambda: build.find_dylib('strandcam', in_path='../../target/release'),
        header_filename=lambda: build.find_header('strandcam.h', in_path='target'),
        rtld_flags=['NOW', 'NODELETE']
    )

setup(
    name='strandcam',
    version='0.0.1',
    packages=find_packages(),
    include_package_data=True,
    zip_safe=False,
    platforms='any',
    install_requires=[
        # keep in sync with `install_requires` in environment.yml
        'milksnake==0.1.5',
        'cffi==1.12.2',
    ],
    milksnake_tasks=[
        build_native,
    ]
)
