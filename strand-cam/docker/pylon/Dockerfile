FROM ubuntu:12.04

RUN apt-get update -y && apt-get dist-upgrade -y

RUN apt-get install build-essential curl pkg-config -y
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y

VOLUME /src

RUN mkdir /pylon
ADD docker/pylon/pylon-5.0.5.9000--RC8-x86_64.tar.gz /pylon
ADD docker/pylon/sha256sums.txt /pylon

RUN cd /pylon && sha256sum -c sha256sums.txt

RUN tar xf /pylon/pylon-5.0.5.9000-x86_64/pylonSDK-5.0.5.9000-x86_64.tar.gz -C /opt

RUN curl -sL https://deb.nodesource.com/setup_6.x | bash -
RUN apt-get install -y nodejs

RUN npm install -g elm
RUN npm install -g create-elm-app

WORKDIR /src/rust-cam/fview2

ENV PATH /root/.cargo/bin:$PATH

CMD ./docker/pylon/inner-run.sh
