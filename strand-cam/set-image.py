#!/usr/bin/python
# note about the above line -- it should really be /usr/bin/python (and not /usr/bin/env python) because
# we need the Apple installed python.

# from http://apple.stackexchange.com/a/161984
import Cocoa
import sys

source = sys.argv[1].decode('utf-8')
dest = sys.argv[2].decode('utf-8')

Cocoa.NSWorkspace.sharedWorkspace().setIcon_forFile_options_(
    Cocoa.NSImage.alloc().initWithContentsOfFile_(source),
    dest,
    0) or sys.exit("Unable to set file icon")
