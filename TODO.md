Items that might be worth exploring or implementing in the future:

- Pass debug mode to ebuild phase execution
- Parse eclass information and calculate digest for proper cache invalidation
- Consider batching metadata cache reads
- Get rid of CPV::qualified_name() allocations
- Batch CPV index insertion and sorting and get rid of key string duplication
