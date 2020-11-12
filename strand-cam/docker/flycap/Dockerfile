FROM ubuntu:12.04

RUN apt-get update -y && apt-get dist-upgrade -y

RUN apt-get install build-essential curl pkg-config -y
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y

RUN apt-get install -y libraw1394-11 libgtk2.0-0 libgtkmm-2.4-dev libglademm-2.4-dev libgtkglextmm-x11-1.2-dev libusb-1.0-0 libglademm-2.4-dev

VOLUME /src

RUN mkdir /flycap
ADD docker/flycap/flycapture2-2.9.3.43-amd64-pkg.tgz /flycap
ADD docker/flycap/sha256sums.txt /flycap

RUN cd /flycap && sha256sum -c sha256sums.txt

RUN cd /flycap/flycapture2-2.9.3.43-amd64 && dpkg -i --force-depends *.deb

RUN apt-get -f install -y

RUN curl -sL https://deb.nodesource.com/setup_6.x | bash -
RUN apt-get install -y nodejs

RUN npm install -g elm
RUN npm install -g create-elm-app

WORKDIR /src/rust-cam/fview2

ENV PATH /root/.cargo/bin:$PATH

CMD ./docker/flycap/inner-run.sh
