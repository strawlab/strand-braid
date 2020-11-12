from setuptools import setup
from setuptools_rust import RustExtension

setup(name='pyci2',
      version='0.1.0',
      rust_extensions=[RustExtension(
            name='pyci2._pyci2',
            path='extensions/Cargo.toml',
            debug=False, # build with --release, even for in-place build
            )],
      packages=['pyci2'],
      zip_safe=False
)

