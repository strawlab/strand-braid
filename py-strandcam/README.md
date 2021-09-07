## Install Python prerequisites

    source $HOME/miniconda3/etc/profile.d/conda.sh
    conda env create -f environment.yml
    conda activate strandcam

## Install and test

    cd ../strand-cam/yew_frontend && ./build.sh
    cd -
    touch rust/build.rs && BACKEND=pyloncxx IPP_SYS=2019 python setup.py install && python scripts/demo.py

## TODO

[ ] allow passing server URL and other args at startup
