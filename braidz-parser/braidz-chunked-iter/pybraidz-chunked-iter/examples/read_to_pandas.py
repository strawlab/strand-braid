import pybraidz_chunked_iter # install with "pip install pybraidz_chunked_iter"
import pandas as pd
import sys

# Get the filename of the braidz file from the command line.
braidz_fname = sys.argv[1]

# Open the braidz file and create chunks of 60 second durations.
estimates_chunker = pybraidz_chunked_iter.chunk_on_duration(braidz_fname, 60)

# One could also create chunks with 100 frames of data.
# estimates_chunker = pybraidz_chunked_iter.chunk_on_num_frames(braidz_fname, 100)

# Iterate over each chunk
for chunk in estimates_chunker:
    print("Read chunk with %d rows"%(chunk["n_rows"],))

    # Create a pandas DataFrame with the data from each chunk
    df = pd.DataFrame(data=chunk["data"])
    print(df)
