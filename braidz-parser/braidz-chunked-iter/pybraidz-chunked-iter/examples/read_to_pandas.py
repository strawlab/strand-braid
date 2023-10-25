import pybraidz_chunked_iter
import pandas as pd
import sys

# Get the filename of the braidz file from the command line.
braidz_fname = sys.argv[1]

# Open the braidz file and create chunks of 60 second durations.
estimates_chunker = pybraidz_chunked_iter.KalmanEstimatesChunker(braidz_fname, 60)

# Iterate over each chunk
for chunk in estimates_chunker:
    print("Read chunk with %d rows"%(chunk["n_rows"],))

    # Create a pandas DataFrame with the data from each chunk
    df = pd.DataFrame(data=chunk["data"])
    print(df)
