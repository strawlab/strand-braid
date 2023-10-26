import pybraidz_chunked_iter # install with "pip install pybraidz_chunked_iter"
import sys

# Get the filename of the braidz file from the command line.
braidz_fname = sys.argv[1]

# Open the braidz file and create chunks of 60 second durations.
estimates_chunker = pybraidz_chunked_iter.chunk_on_duration(braidz_fname, 60)

# Iterate over each chunk
for chunk in estimates_chunker:
    print(chunk)
